---
description: "Use when working on crv-edge daemon, workspace management, file watching, system tray, or RocksDB storage."
applyTo: "crv-edge/**"
---
# crv-edge Daemon Conventions

## Role
用户端守护进程，提供工作区管理、文件变更监视等功能。监听 `[::1]:34562`（IPv6 localhost）。

## Stack
- gRPC (tonic)：接收来自 crv-cli 的命令
- iroh：与 crv-hive 通信
- RocksDB：本地状态存储
- tokio async runtime

## Platform Features
- Windows：系统托盘图标 (tray-icon + tao)、开机自启 (winreg)
- 跨平台核心逻辑通过 `#[cfg(target_os)]` 条件编译
