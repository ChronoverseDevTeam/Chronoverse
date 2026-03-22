# Chronoverse
![title-image](./title.webp)
Chronoverse — A centralized, heavy-asset VCS designed for the gaming and film industry. Empowering creators with reliable version control, seamless collaboration, and scalable asset management.

## 项目架构

Chronoverse 采用分布式架构，包含以下组件：

### 核心组件

- **crv-core** - 核心库，提供基础功能
- **crv-hive** - 中央服务器，管理分布式节点
- **crv-edge** - 边缘节点守护进程
- **crv-cli** - 命令行工具

### 协议定义

- **proto/daemon.proto** - crv-edge (daemon) 的 gRPC 协议定义
- **proto/hive.proto** - crv-hive 的 gRPC 协议定义

### 服务端口

- **crv-hive**: `0.0.0.0:34560` - 中央服务器
- **crv-edge**: `http://[::1]:31822` - 边缘节点守护进程 (默认端口，可通过 bootstrap 配置调整)

## 快速开始

推荐启动顺序：先 **crv-hive**，再 **crv-edge**，最后运行 **crv-cli**。

### 启动中央服务器

```bash
cd crv-hive
cargo run
```

### 启动边缘节点

```bash
cd crv-edge
cargo run
```

### 使用命令行工具

```bash
cd crv-cli
cargo run
```

无参数或加 `--repl` 会进入交互式 shell；也可直接传子命令，例如：

- `cargo run -- login` (交互式输入密码；需要 hive 可达)
- `cargo run -- whoami`
- `cargo run -- workspace list`
- `cargo run -- logout`

## Discord
Join [Discord Server](https://discord.gg/yfC9TtMc) to develop this project with us.
