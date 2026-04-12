# Chronoverse Project Guidelines

## Overview

Chronoverse (CRV) 是一个对标 Perforce P4 的开源版本控制系统，专为游戏和影视行业的大型重资产工程设计。采用中心化架构，由 Rust 后端 + React TypeScript 前端组成。

## Architecture

5 大模块，各自独立 crate：

| 模块 | 角色 | 对标 P4 |
|------|------|---------|
| **crv-cli** | 命令行客户端，通过 gRPC 与 edge 通信 | p4 命令行 |
| **crv-edge** | 用户端守护进程，工作区管理、文件变更监视 | p4 客户端守护 |
| **crv-hive** | 中心化服务器，版本控制核心服务 | p4d |
| **crv-relay** | iroh 中继服务 | — |
| **crv-ui** | React 图形界面，与 edge 通信 | P4V |

hive 和 edge 基于 iroh 框架通信，本质上都是 iroh 端点。CLI 和 edge 之间通过 gRPC 通信。

## Key Dependencies

- **Runtime**: tokio (async), serde/bincode (序列化)
- **Networking**: iroh 0.97 (hive↔edge), tonic/gRPC (cli↔edge)
- **Storage**: sea-orm + PostgreSQL (hive), rocksdb (edge)
- **CAS**: BLAKE3 哈希, LZ4 压缩, 256-shard 目录结构
- **UI**: React 19, Vite, Tailwind CSS 4

## Build & Run

```bash
# 检查编译
cargo check -p crv-hive
cargo check -p crv-edge
cargo check -p crv-cli

# 运行
cargo run -p crv-hive    # 中心服务器 (0.0.0.0:34560)
cargo run -p crv-edge    # 守护进程 ([::1]:34562)
cargo run -p crv-cli     # 命令行

# 前端
cd crv-ui && npm run dev
```

Proto 文件位于 `proto/`，由 `build.rs` 中的 `tonic-prost-build` 自动编译。

## Code Conventions

- 错误处理：库 crate 用 `thiserror` 自定义错误枚举 + `#[from]`；二进制 crate 用 `anyhow::Result`
- 命名：snake_case 函数/变量，PascalCase 类型/结构体/特征
- 异步：全面使用 tokio async/await
- Depot 路径语法：`//path/to/file`（单文件）、`//path/...~ext`（递归通配）、`r://regex`（正则）
- 测试：标准 `#[cfg(test)]` 模块 + `#[test]`

## Documentation References

- 数据层规范：`crv-core/src/repository/RFC_CRV.md`（已废弃，现在使用 iroh 提供的 blobs 服务）
- 元数据规范：`crv-core/src/metadata/RFC.md`（已废弃，参照代码实现）
