# Chronoverse CLI 使用指南

## 概述

Chronoverse CLI (`crv-cli`) 是一个版本控制命令行工具，支持两种运行模式：
- **gRPC 模式**：连接到远程 `crv-edge` 守护进程
- **本地模拟模式**：在本地模拟客户端-服务器交互，用于测试和开发

## 启动方式

### 本地模拟模式（推荐用于测试）

```bash
cd crv-cli
cargo run -- --local
```

**可选参数**：
```bash
# 指定工作空间和服务器根目录
cargo run -- --local --workspace ./my_workspace --server-root ./my_server
cargo run -- -l -w ./my_workspace --server-root ./my_server
```

### gRPC 模式（连接真实服务器）

```bash
# 使用默认服务器地址 (http://127.0.0.1:34562)
cargo run

# 指定服务器地址
cargo run -- --server http://192.168.1.100:34562
cargo run -- -s http://192.168.1.100:34562
```

### gRPC + 本地模拟模式

```bash
# 同时连接服务器并启用本地模拟
cargo run -- --local --server http://127.0.0.1:34562
```

此模式下，每个命令会：
1. 发送 gRPC 请求到服务器并打印回包
2. 执行本地模拟逻辑
3. 返回本地模拟结果

---

## 交互式命令

启动后，程序进入交互模式，可以连续输入命令。

### 基础命令

#### `help` - 显示帮助

```bash
crv> help
```

显示所有可用命令的列表。

#### `exit` / `quit` - 退出

```bash
crv> exit
```

退出交互式命令行。

---

### Edge 命令（版本控制）

#### `edge ping` - 测试连接

```bash
crv> edge ping
```

测试与服务器的连接并显示服务器信息（版本、平台、架构等）。

#### `edge create-workspace` - 创建工作空间

```bash
crv> edge create-workspace
```

创建工作空间。在本地模拟模式下，会自动初始化示例数据：
- `file1.txt` (3个版本)
- `file2.txt` (2个版本)
- `docs/readme.md` (4个版本)

#### `edge get-latest` - 获取文件列表

```bash
crv> edge get-latest
```

获取服务器上所有文件的最新版本列表。

**示例输出**：
```
服务器上的文件列表 (3 个文件):
  1. file1.txt
  2. file2.txt
  3. docs/readme.md
```

#### `edge checkout <FILE>` - 检出文件

```bash
crv> edge checkout file1.txt
crv> edge checkout docs/readme.md
```

从服务器检出文件的最新版本到本地工作空间。

#### `edge get-revision <FILE> -r <REVISION>` - 切换版本

```bash
crv> edge get-revision file1.txt -r 1
crv> edge get-revision file1.txt -r 2
```

将本地文件切换到指定版本。

**注意**：此命令仅在本地模拟模式（`--local`）下可用。

#### `edge submit <FILE> -d <DESCRIPTION>` - 提交文件

```bash
crv> edge submit file1.txt -d "修复bug"
crv> edge submit src/main.rs -d "添加新功能"
```

将本地修改的文件提交到服务器，创建新版本。

---

### Workspace 命令

#### `workspace list` - 列出工作区

```bash
crv> workspace list
```

列出所有可用的工作区（功能待实现）。

---

## 完整使用示例

### 示例 1：本地模拟模式工作流

```bash
# 1. 启动 CLI（本地模拟模式）
cd crv-cli
cargo run -- --local

# 2. 在交互界面中执行命令
crv> help
# 查看所有可用命令

crv> edge create-workspace
# 创建工作空间并初始化示例数据
# 输出：
#   📦 Initializing server with sample data...
#   ✓ Created file1.txt with 3 versions
#   ✓ Created file2.txt with 2 versions
#   ✓ Created docs/readme.md with 4 versions

crv> edge get-latest
# 获取文件列表
# 输出：file1.txt, file2.txt, docs/readme.md

crv> edge checkout file1.txt
# 检出文件到本地工作空间
# 输出：✅ Checked out file1.txt revision 3 to "workspace/file1.txt"
```

**在另一个终端查看文件**：
```bash
cat workspace/file1.txt
# 应该看到：Version 3 of file1
#           Final version with updates
```

**继续在交互界面中测试版本切换**：
```bash
crv> edge get-revision file1.txt -r 1
# 切换到版本 1
```

**查看文件内容**：
```bash
cat workspace/file1.txt
# 应该看到：Version 1 of file1
#           Initial content
```

**切换到版本 2**：
```bash
crv> edge get-revision file1.txt -r 2
```

**查看文件内容**：
```bash
cat workspace/file1.txt
# 应该看到：Version 2 of file1
#           Added more content
```

**修改并提交新版本**：
```bash
# 在另一个终端修改文件
echo "My new version
New features added" > workspace/file1.txt

# 在交互界面提交
crv> edge submit file1.txt -d "Updated content"
# 输出：✅ Submitted file1.txt as revision 4 (changelist 10)
```

**退出**：
```bash
crv> exit
```

---

### 示例 2：gRPC 模式工作流

**终端 1 - 启动 crv-edge 服务器**：
```bash
cd crv-edge
cargo run
```

**终端 2 - 启动 CLI 客户端**：
```bash
cd crv-cli
cargo run

# 在交互界面中执行
crv> edge ping
# 测试连接，显示服务器信息

crv> edge create-workspace
# 在服务器上创建工作空间

crv> edge get-latest
# 获取服务器上的文件列表

crv> edge checkout file1.txt
# 从服务器检出文件

crv> edge submit file1.txt -d "My changes"
# 提交修改到服务器

crv> exit
```

---

### 示例 3：gRPC + 本地模拟模式

**终端 1 - 启动服务器**：
```bash
cd crv-edge
cargo run
```

**终端 2 - 启动客户端（本地模拟）**：
```bash
cd crv-cli
cargo run -- --local

# 在交互界面中执行
crv> edge create-workspace
# 输出会包含：
#   📦 gRPC 回包: success=true, message=工作空间已创建, path=...
#   ✅ 本地模拟工作空间已创建

crv> edge get-latest
# 输出会包含：
#   📦 gRPC 回包: success=true, files=[...]
#   服务器上的文件列表 (3 个文件): ...
```

此模式下可以同时看到服务器的实际响应和本地模拟的结果。

---

## 命令行参数

| 参数 | 短选项 | 默认值 | 说明 |
|------|--------|--------|------|
| `--server` | `-s` | `http://127.0.0.1:34562` | gRPC 服务器地址 |
| `--local` | `-l` | `false` | 启用本地模拟模式 |
| `--workspace` | `-w` | `./workspace` | 本地工作空间根目录 |
| `--server-root` | | `./server` | 本地模拟服务器根目录 |

---

## 故障排除

### 连接失败

如果在 gRPC 模式下看到连接错误：

```
错误: tonic::transport::Error(Transport, ConnectError(...))
```

**解决方案**：
1. 确认 `crv-edge` 守护进程正在运行
2. 检查服务器地址是否正确
3. 或者使用本地模拟模式：`cargo run -- --local`

### 文件不存在

如果提示文件不存在：
1. 先运行 `edge create-workspace` 初始化示例数据（本地模拟模式）
2. 使用 `edge get-latest` 查看服务器上的可用文件

### get-revision 命令不可用

`edge get-revision` 命令仅在本地模拟模式下可用。如果看到错误：

```
错误: get-revision 仅在本地模拟模式下可用
```

**解决方案**：使用 `--local` 标志启动：
```bash
cargo run -- --local
```

---

## 开发者信息

- **项目**：Chronoverse
- **版本**：0.1.0
- **许可**：见 LICENSE 文件
- **仓库**：https://github.com/your-repo/chronoverse
