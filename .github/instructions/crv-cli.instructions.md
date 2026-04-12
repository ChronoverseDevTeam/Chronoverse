---
description: "Use when working on crv-cli commands, Clap argument parsing, or gRPC client calls to edge daemon."
applyTo: "crv-cli/**"
---
# crv-cli Conventions

## Role
命令行工具，通过 gRPC 与 crv-edge 通信，提供类似 p4 命令行的功能。

## Stack
- Clap 4：命令行参数解析
- tonic gRPC：与 edge 通信
- dialoguer / indicatif / console / tabled：交互式 UI 和输出美化

## Structure
- 命令定义在 `src/commands/` 下，每个子命令一个文件
- 业务逻辑在 `src/logic/`
- 入口 `src/main.rs`

## Depot Path Syntax
- 单文件：`//path/to/file`
- 递归通配：`//path/...~ext`
- 正则：`r://regex`
