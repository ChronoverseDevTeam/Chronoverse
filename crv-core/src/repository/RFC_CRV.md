# 📄 **RFC：Chronoverse 数据层规范（Chronoverse Data Layer Specification）**

**版本：1.1（v1.1）**
**状态：Draft / 草案**

---

# 目录

1. 引言
2. 术语说明
3. 系统总体架构
4. Chunk 标识
5. 文件系统布局
6. Pack 文件（`.dat`）格式
7. Index 文件（`.idx`）格式
8. 文件生命周期与封存（Sealing）语义
9. 未封存索引文件（Mutable Index）规范
10. 错误处理
11. 兼容性
12. 安全性

---

# 1. 引言

本 RFC 定义 Chronoverse 系统使用的 **内容寻址存储（CAS）格式**，包括：

* 文件块（Chunk）的唯一标识方式
* Pack 文件的二进制结构
* 可增量更新的 Index 文件格式
* Pack / Index 的封存流程
* 数据校验与错误处理规则

本规范旨在确保不同实现之间的互操作性与完整性。

---

# 2. 术语说明

* **Chunk**：文件块，一段原始字节数据。
* **Chunk Hash**：未压缩数据的 32 字节 BLAKE3 哈希。
* **Pack File（`.dat`）**：Chunk 数据主体文件，可 append。
* **Index File（`.idx`）**：索引文件，提供 hash → offset/length 映射。
* **未封存（Open）**：可修改状态。
* **已封存（Sealed）**：不可修改状态，文件尾部带 CRC。
* **LE**：Little Endian，小端序。

除非特别说明，所有 u16/u32/u64 字段均采用 **小端序**。

---

# 3. 系统总体架构

系统由如下部分组成：

* 256 个分片目录（shard-00 ～ shard-FF）
* 每个分片包含多个 pack
* 每个 pack 由 `.dat` 与 `.idx` 组成
* `.dat` 可追加写（append-only）
* `.idx` 在未封存时可动态插入，保持排序
* 封存后 `.dat` 与 `.idx` 均不可变

---

# 4. Chunk 标识

Chunk ID 定义为：

> **BLAKE3(原始未压缩字节流) → 32 字节哈希**

哈希必须视为全局唯一。
若系统检测到：

> 同一哈希值对应不同数据 → 必须视为**严重数据损坏**。

分片规则：

```
shard_id = chunk_hash[0]
```

---

# 5. 文件系统布局

```
data/
  shard-00/
    pack-000001.dat
    pack-000001.idx
    pack-000002.dat
    pack-000002.idx
  shard-01/
  ...
  shard-FF/
```

文件命名无固定语义，可递增编号。

---

# 6. Pack 文件（`.dat`）格式

Pack 文件为：

```
+---------------------------------------------------------------+
| PACK HEADER | CHUNK ENTRY... | PACK END (可选，仅封存后存在) |
+---------------------------------------------------------------+
```

## 6.1 Pack Header

| 字段       | 类型  | 值 / 说明                |
| -------- | --- | --------------------- |
| magic    | u32 | `"CRVB"` = 0x43525642 |
| version  | u16 | 0x0001                |
| reserved | u32 | 保留，必须为 0              |

大小固定 **10 字节**。

---

## 6.2 Chunk Entry

二进制布局如下：

```
+0   u32       chunk_data_len
+4   u16       flags
+6   u8[32]    hash
+38  u8[...]   chunk_data（长度 = chunk_data_len）
```

总长度：`38 + chunk_data_len`。

### flags 定义

| 位          | 含义                   |
| ---------- | -------------------- |
| bit0       | 0 = 未压缩 / 1 = LZ4 压缩 |
| bit1~bit15 | MUST = 0，读取时忽略未知位    |

`hash` 必须是 **未压缩 chunk 原始数据的 BLAKE3**。

---

## 6.3 Pack End（封存）

封存后在文件末尾追加：

| 字段    | 类型  | 描述                      |
| ----- | --- | ----------------------- |
| CRC32 | u32 | 对整个文件（不含CRC字段）计算的 CRC32 |

未封存 pack 文件不得包含此字段。

---

# 7. Index 文件（`.idx`）格式

Index 文件用于快速定位 `.dat` 中的 Chunk，结构：

```
+------------------------------------------------+
| IDX HEADER | IDX ENTRY... | IDX END (封存后)  |
+------------------------------------------------+
```

## 7.1 索引保持排序（关键要求）

Index 文件中所有条目必须：

> **按 hash 的字节序升序排序（lexicographical ordering）**

这是查找性能的保证。

---

## 7.2 Index Header

| 字段          | 类型  | 描述                    |
| ----------- | --- | --------------------- |
| magic       | u32 | `"CRVI"` = 0x43525649 |
| version     | u16 | 0x0001                |
| reserved    | u32 | MUST=0，读取时忽略          |
| entry_count | u64 | 当前索引条目数量              |

Header 长度 **18 字节**。

---

## 7.3 Index Entry

| 字段     | 类型     | 描述                                          |
| ------ | ------ | ------------------------------------------- |
| hash   | u8[32] | Chunk 哈希                                    |
| offset | u64    | Chunk Entry 在 `.dat` 中的byte偏移（含pack header） |
| length | u32    | chunk_data_len                              |
| flags  | u16    | 与 `.dat` 中一致                                |

`offset` 必须指向 Chunk Entry 起始处。

---

## 7.4 Index End（封存）

封存后追加：

| 字段    | 类型  | 描述                       |
| ----- | --- | ------------------------ |
| CRC32 | u32 | 对全 idx 文件（不含自身）计算的 CRC32 |

未封存 idx 文件不得包含此字段。

---

# 8. 文件生命周期与封存（Sealing）

## 8.1 Pack / Index 的状态

| 状态  | `.dat` | `.idx` | CRC | 可写性 |
| --- | ------ | ------ | --- | --- |
| 未封存 | 可写     | 可写且可更新 | 无   | 可变  |
| 已封存 | 只读     | 只读     | 有   | 不可变 |

---

## 8.2 封存流程（Sealing）

封存一个 pack 的步骤：

1. 停止向 `.dat` 写入
2. 校验 `.idx`：

   * entry_count 合法
   * hash 升序
3. 将 CRC32 写入 `.idx`
4. 将 CRC32 写入 `.dat`
5. 将文件权限标记为只读
6. pack 进入 Sealed 状态

---

# 9. 未封存索引文件（Mutable Index）规范（v1.1 新增）

`.idx` 文件在 pack 处于未封存状态时必须支持更新，包括：

---

## 9.1 创建

打开新 pack 时：

* 创建 `.idx` 文件
* 写入 header，entry_count = 0
* 无 CRC32

---

## 9.2 插入新条目（Add Chunk）

当新 Chunk 写入 `.dat` 后，系统必须执行：

1. 计算 hash
2. 确定 `.idx` 插入点（二分查找）
3. 在 IDX 文件中间插入新 entry（文件级别的 shift/insertion）
4. entry_count++
5. 更新 header 中的 entry_count

IDX 必须保持严格排序。

---

## 9.3 未封存 idx 文件的有效性要求

Reader 读取未封存 idx 时必须保证：

* 文件大小 ≥ header + entry_count × entry_size
* 条目按 hash 升序排序
* offset 和 length 必须在 `.dat` 合法范围内
* 任何校验失败视为损坏

未封存 idx 无需 CRC 校验。

---

## 9.4 未封存 idx 的恢复机制（推荐）

如果未封存 idx 被部分写入或损坏，系统应提供恢复逻辑：

* 从 `.dat` 重新扫描所有 entry
* 重新排序
* 重建 `.idx`（无 CRC）

---

# 10. 错误处理

系统必须检测以下错误：

* CRC 校验失败 → 文件损坏
* hash 校验失败 → 严重损坏
* idx hash 非升序 → 索引损坏
* offset 越界 → 索引损坏
* entry_count 不匹配 → 索引损坏

损坏的 pack 或 idx 必须拒绝使用。

---

# 11. 兼容性

本 v1.1 格式兼容 v1.0 的格式（仅添加索引可更新语义）。
未来版本可在：

* flags
* reserved
* version

中扩展字段。

所有：

* reserved 字段必须写 0
* reader 必须忽略 unknown bits（向前兼容）

---

# 12. 安全性

系统必须确保：

* offset 不得导致越界读
* length 不得超过 entry 实际长度
* hash 校验失败必须拒绝
* 未封存 idx 必须谨慎处理 partial-write