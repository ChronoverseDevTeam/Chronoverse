待办清单：
1. 上传的文件块支持压缩算法，而不是现在只能传 none
2. 下载的文件块支持压缩算法，现在也是默认 none 的

## gRPC-Web 支持
- `hive_address` 指定的端口现同时支持 gRPC 与 gRPC-Web（HTTP/1.1）
- 默认开启宽松 CORS，便于浏览器前端（如 http://localhost:5173）直接调用
- 客户端可使用 grpc-web / connect-web，目标地址示例：`http://<hive_host>:<port>`