---
description: "Use when writing or modifying Rust code in any crv-* crate. Covers error handling, async patterns, module conventions, and CAS data layer patterns."
applyTo: "**/*.rs"
---
# Rust Conventions for Chronoverse

## Error Handling
- 库 crate (crv-core)：使用 `thiserror` 定义具体错误枚举，通过 `#[from]` 实现自动转换
- 二进制 crate (crv-cli, crv-edge, crv-hive, crv-relay)：使用 `anyhow::Result<T>` 传播错误
- 不要在库代码中使用 `unwrap()` 或 `expect()`

## Async Patterns
- 运行时：tokio "full" feature
- 所有 I/O 操作必须 async
- 使用 `tokio::spawn` 而非 `std::thread::spawn`
- channel 通信优先使用 `tokio::sync::mpsc`

## Module Organization
- 每个子模块有 `mod.rs` 作为公共接口，内部实现分散到独立文件
- 公共类型通过 `mod.rs` 中的 `pub use` 重导出

## CAS Data Layer (crv-core)
- 所有 chunk 通过未压缩数据的 BLAKE3 哈希标识（32 字节）
- 256-shard 目录结构：`shard-00` 到 `shard-FF`，由 hash[0] 决定分片
- Pack 文件：`.dat`（追加写入）+ `.idx`（可变索引），seal 后不可变
- 压缩：LZ4

## gRPC (cli ↔ edge)
- Proto 定义在 `proto/` 目录
- 使用 `tonic-prost-build` 在 `build.rs` 中编译
- 生成代码输出到 `target/generated/`

## iroh (edge ↔ hive)
- hive 和 edge 均为 iroh 端点
- 使用 iroh-blobs 进行大文件传输
- 中继通过 crv-relay 提供
