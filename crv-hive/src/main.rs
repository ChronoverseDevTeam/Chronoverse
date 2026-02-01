use crv_hive::{config, hive_server, database};
use std::net::SocketAddr;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    crv_hive::logging::init_logging();

    config::holder::load_config().await?;

    database::init().await?;

    let addr_str = config::holder::get_config()
        .unwrap()
        .hive_address
        .clone()
        .unwrap_or_else(|| "0.0.0.0:34560".to_string());
    let addr: SocketAddr = addr_str
        .parse()
        .expect(&format!("unable to parse addr `{}`", addr_str));

    println!("Hive gRPC / gRPC-Web service is available at {}", addr);

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

        if let Err(e) = database::shutdown().await {
            eprintln!("failed to shutdown database: {e}");
        }
    };

    // Launching
    hive_server::start_server_with_shutdown(addr, shutdown).await
}
