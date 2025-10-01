pub mod proto_server;
pub mod utils;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/daemon_proto.rs"));
}