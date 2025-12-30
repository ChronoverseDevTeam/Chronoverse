pub mod client_manager;
pub mod daemon_server;
pub mod utils;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/daemon_proto.rs"));
}

pub mod hive_pb {
    include!(concat!(env!("OUT_DIR"), "/hive_proto.rs"));
}
