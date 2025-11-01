# Chronoverse CLI 使用指南

## 概述

Chronoverse CLI (`crv-cli`) 是一个版本控制命令行工具，支持两种运行模式：
- **gRPC 模式**：连接到远程 `crv-edge` 守护进程
- **本地模拟模式**：在本地模拟客户端-服务器交互，用于测试和开发

## 启动方式

## 完整使用示例

### 示例：gRPC + 本地模拟 + Hive 集成

**终端 1 - 启动 crv-hive 服务器**：
```bash
cd crv-hive
cargo run
# Hive 服务器将在 0.0.0.0:34560 上监听
```

**终端 2 - 启动 crv-edge 守护进程**：
```bash
cd crv-edge
cargo run
# Edge 守护进程将在 127.0.0.1:34562 上监听
```

**终端 3 - 启动 crv-cli 客户端（本地模拟模式）**：
```bash
cd crv-cli
cargo run -- --local
```

**终端 4 - 启动 MongoDB 服务**：
```bash
cd crv-hive/mongo

# 方法 1：自动下载 MongoDB（如果本地没有）
.\native_start.ps1

# 方法 2：使用本地已安装的 MongoDB
.\native_start.ps1 -MongodPath "C:\Program Files\MongoDB\Server\8.2\bin\mongod.exe"
```


# ========== 1. Edge 基础功能测试 ==========

```bash
crv> edge ping
# 测试与 Edge 守护进程的连接
# 输出：
#   📦 gRPC 回包: version=1.0.0, api_level=1, platform=chronoverse
#   收到服务器信息:
#     守护进程版本: 1.0.0
#     API 级别: 1
#     平台: chronoverse
#     操作系统: windows
#     架构: x86_64


crv> edge create-workspace
# 创建工作空间并初始化示例数据
# 输出：
#   📦 gRPC 回包: success=true, message=工作空间已创建, path=...
#   📦 Initializing server with sample data...
#   ✓ Created file1.txt with 3 versions
#   ✓ Created file2.txt with 2 versions
#   ✓ Created docs/readme.md with 4 versions
#   ✅ 本地模拟工作空间已创建

crv> edge get-latest
# 获取文件列表
# 输出：
#   📦 gRPC 回包: success=true, files=[...]
#   服务器上的文件列表 (3 个文件):
#     1. file1.txt
#     2. file2.txt
#     3. docs/readme.md

crv> edge checkout file1.txt
# 检出文件到本地工作空间
# 输出：
#   📦 gRPC 回包: success=true, message=模拟检出文件: file1.txt
#   ✅ Checked out file1.txt revision 3 to "workspace/file1.txt"

crv> edge get-revision file1.txt -r 1
# 切换到版本 1（仅本地模拟模式支持）
# 输出：
#   正在切换到版本 1 of file1.txt
#   ✅ Checked out file1.txt revision 1 to "workspace/file1.txt"

crv> edge submit file1.txt -d "Updated content"
# 提交修改到服务器
# 输出：
#   📦 gRPC 回包: success=true, message=模拟提交变更列表...
#   ✅ Submitted file1.txt as revision 4 (changelist 10)

# ========== 2. Hive 集成功能测试 ==========

crv> edge hive-connect
# 连接到 Hive 服务器（默认 http://127.0.0.1:34560）
# 输出：
#   正在连接到 Hive 服务器: http://127.0.0.1:34560
#   ✅ 已连接到 Hive 服务器

crv> edge hive-register -u alice -p password123 -e alice@example.com
# 注册新用户
# 输出：
#   正在注册用户: alice
#   ✅ 用户 'alice' 注册成功！

crv> edge hive-login -u alice -p password123
# 登录到 Hive
# 输出：
#   正在登录用户: alice
#   ✅ 登录成功！
#     Access Token: eyJhbGciOiJIUzI1NiIsInR...
#     Expires At: 1730476800

crv> edge hive-list-workspaces -n my_workspace -o alice -d device123
# 列出 Hive 上的所有工作空间
# 输出：
#   正在获取工作空间列表...
#   📋 工作空间列表 (2 个):
#     1. workspace1 (owner: alice, path: /path/to/ws1)
#     2. workspace2 (owner: alice, path: /path/to/ws2)

crv> exit
# 退出
```