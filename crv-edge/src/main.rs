use std::net::SocketAddr;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "127.0.0.1:34562".parse()?;
    
    println!("Starting gRPC server on {}", addr);

    // Ctrl+C 优雅关闭触发器
    let shutdown = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        println!("\nReceived CTRL+C signal, shutting down gracefully...");
    };

    // 使用支持优雅关闭的启动函数
    crv_edge::proto_server::server_entry::start_server_with_shutdown(addr, shutdown).await
}