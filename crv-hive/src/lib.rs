// 将通过 build.rs 生成到 OUT_DIR 的 protobuf 模块引入并导出
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/hive_proto.rs"));
}

pub mod config;
pub mod auth;
pub mod hive_server;
pub mod database;
pub mod caching;
pub mod common;