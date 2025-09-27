// 将通过 build.rs 生成到 OUT_DIR 的 protobuf 模块引入并导出
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/server_proto.rs"));
}

pub mod client;
