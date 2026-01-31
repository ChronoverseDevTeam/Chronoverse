待办清单：
1. 上传的文件块支持压缩算法，而不是现在只能传 none
2. 下载的文件块支持压缩算法，现在也是默认 none 的
3. P0：upload_file_chunk 没鉴权/没校验 ticket 归属（上面第 4 点）——存在越权面。
4. P1：全局 chunk cache + 无条件 delete（上面第 2 点）——会出现跨 ticket 干扰，且安全边界模糊。
5. P1：upload 阶段不做超时/清理触发——过期锁释放依赖“有人来 submit/launch”。

## gRPC-Web 支持
- `hive_address` 指定的端口现同时支持 gRPC 与 gRPC-Web（HTTP/1.1）
- 默认开启宽松 CORS，便于浏览器前端（如 http://localhost:5173）直接调用
- 客户端可使用 grpc-web / connect-web，目标地址示例：`http://<hive_host>:<port>`