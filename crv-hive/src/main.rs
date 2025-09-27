use std::net::SocketAddr;
use tokio::signal;
use crv_hive::hive_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:34560".parse()?;
    
    println!("Starting Hive gRPC server on {}", addr);

    // Ctrl+C 优雅关闭触发器
    let shutdown = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        println!("\nReceived CTRL+C signal, shutting down gracefully...");
    };

    // 使用支持优雅关闭的启动函数
    hive_server::start_server_with_shutdown(addr, shutdown).await
}