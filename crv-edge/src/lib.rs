pub mod proto_server;

// 将通过 build.rs 生成到 OUT_DIR 的 protobuf 模块引入并导出
pub mod pb {
    // 由于 client.proto 与 server.proto 共享同一个 package `helloworld`
    // prost 会把它们合并生成为一个 helloworld.rs 文件
    include!(concat!(env!("OUT_DIR"), "/server_proto.rs"));
}