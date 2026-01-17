# Chronoverse CLI 使用指南

## 示例

### 启动

**终端 1 - 启动 crv-hive 服务器**：
```bash
cd crv-hive
cargo run
```

**终端 2 - 启动 crv-edge 守护进程**：
```bash
cd crv-edge
cargo run
```

**终端 3 - 启动 crv-cli 客户端**：
```bash
cd crv-cli
cargo run
```

**终端 4 - 启动 MongoDB 服务**：
```bash
cd crv-hive/mongo

# 方法 1：自动下载 MongoDB（如果本地没有）
.\native_start.ps1

# 方法 2：使用本地已安装的 MongoDB
.\native_start.ps1 -MongodPath "C:\Program Files\MongoDB\Server\8.2\bin\mongod.exe"
```


### 指令

```bash
crv> workspace create

# name: test
# root: D:\test
# mapping: //a/b/c/... //test/
# 在D:\test下添加test.txt

crv> workspace add --workspace test //test/test.txt

# 查看add delete checkout情况
crv> showactive --workspace test //test/
```