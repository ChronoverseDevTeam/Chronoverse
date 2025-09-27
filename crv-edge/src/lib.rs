pub mod proto_server;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/daemon_proto.rs"));
}