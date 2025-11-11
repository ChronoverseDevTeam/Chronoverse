use std::net::SocketAddr;
use tokio::signal;
use crv_hive::{config, hive_server, database, s3client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    config::holder::load_config().await?;

    let addr_str = config::holder::get_config()
        .unwrap()
        .hive_address
        .clone()
        .unwrap_or_else(|| "0.0.0.0:34560".to_string());
    let addr: SocketAddr = addr_str
        .parse()
        .expect(&format!("unable to parse addr `{}`", addr_str));

    database::mongo::init_mongo_from_config().await?;
    
    // 初始化 S3 客户端
    s3client::init_s3_client().await?;

    println!("Hive gRPC sevice now is available at {}", addr);

    // Ctrl+C to shutdown gracefully
    let shutdown = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        println!("\nReceived CTRL+C signal, shutting down gracefully...");

        // Flush config and close database handles
        if let Err(e) = config::holder::shutdown_config().await {
            eprintln!("failed to save config on shutdown: {}", e);
        }
        database::mongo::shutdown_mongo().await;
    };

    // Launching
    hive_server::start_server_with_shutdown(addr, shutdown).await
}