待办清单：
1. 上传的文件块支持压缩算法，而不是现在只能传 none
2. 下载的文件块支持压缩算法，现在也是默认 none 的
3. P0：upload_file_chunk 没鉴权/没校验 ticket 归属（上面第 4 点）——存在越权面。
4. P1：全局 chunk cache + 无条件 delete（上面第 2 点）——会出现跨 ticket 干扰，且安全边界模糊。
5. P1：upload 阶段不做超时/清理触发——过期锁释放依赖“有人来 submit/launch”。
6. 把 cache 按 ticket 隔离（目录或命名空间），或做引用计数/TTL，而不是全局按 hash 共享并在解锁时直接删除。

