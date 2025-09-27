# crv-cli

Chronoverse CLI - 用于连接 crv-edge 守护进程的命令行接口

## 项目结构

项目使用共享的 proto 文件结构：
- `proto/server.proto` - 共享的 gRPC 协议定义文件
- `crv-edge` - gRPC Daemon 实现
- `crv-cli` - gRPC Cli 工具实现

## 功能

- 连接到 crv-edge gRPC 服务器
- 发送问候消息到服务器
- 支持自定义服务器地址和消息内容

## 使用方法

### 基本用法

```bash
# 使用默认设置连接服务器
cargo run -- connect

# 查看帮助信息
cargo run -- --help
cargo run -- connect --help
```

### 自定义参数

```bash
# 指定服务器地址
cargo run -- connect --server http://127.0.0.1:34562

# 指定自定义消息
cargo run -- connect --message "我的自定义消息"

# 同时指定服务器地址和消息
cargo run -- connect --server http://127.0.0.1:34562 --message "测试消息"
```

## 前提条件

在使用 crv-cli 之前，需要先启动 crv-edge 服务器：

```bash
cd ../crv-edge
cargo run
```

服务器默认运行在 `127.0.0.1:34562`。

## 构建

```bash
cargo build
```

## 运行

```bash
cargo run -- connect
```
