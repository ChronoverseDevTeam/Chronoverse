use std::sync::Arc;

use crv_hive::crv2::{
    ChronoverseApp,
    iroh::{
        iroh_client::{IrohClient, IrohClientConfig, ALPN},
        relay::RelayServer,
        service::IrohService,
    },
};
use tokio::io::{AsyncBufReadExt, BufReader};

// ── Test helpers ──────────────────────────────────────────────────────────────

/// Start an embedded relay on an ephemeral port and a bound server iroh
/// endpoint.  Returns `(relay, relay_url, server_client)`.
async fn start_test_server() -> (RelayServer, String, IrohClient) {
    let relay = RelayServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("relay start");

    let relay_port = relay.http_addr().expect("relay has http addr").port();
    let relay_url = format!("http://127.0.0.1:{relay_port}");

    let server = IrohClient::start(IrohClientConfig {
        relay_url: Some(relay_url.parse().unwrap()),
        secret_key: None,
    })
    .await
    .expect("server iroh start");

    (relay, relay_url, server)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

/// Connect an iroh client to the [`IrohService`] and call `register_user`.
///
/// Test flow:
/// 1. Start an embedded relay on a random port.
/// 2. Start a server-side iroh endpoint and wrap it in [`IrohService`].
/// 3. Start a client-side iroh endpoint and establish a connection.
/// 4. Open a bi-directional stream, send a `register_user` JSON request.
/// 5. Read the JSON response and assert the returned username.
#[tokio::test]
async fn test_register_user_over_iroh() {
    let (relay, relay_url, server) = start_test_server().await;
    // Save the server address before moving `server` into the service.
    let server_addr = server.addr();

    // Wrap the server endpoint in the service and run it in a background task.
    let app = Arc::new(ChronoverseApp::new());
    let service = IrohService::new(server, app);
    tokio::spawn(async move { service.serve().await });

    // Yield once so the service task has a chance to call accept().
    tokio::task::yield_now().await;

    // Create a client endpoint connected to the same relay.
    let client = IrohClient::start(IrohClientConfig {
        relay_url: Some(relay_url.parse().unwrap()),
        secret_key: None,
    })
    .await
    .expect("client iroh start");

    // Establish a QUIC connection to the server (routed through the relay).
    let conn = client.connect(server_addr, ALPN).await.expect("connect to server");

    // Open a bi-directional stream.
    let (mut send, recv) = conn.open_bi().await.expect("open_bi");

    // Send a register_user request as a newline-terminated JSON line.
    let req = concat!(
        r#"{"method":"register_user","username":"alice","password":"secret"}"#,
        "\n"
    );
    send.write_all(req.as_bytes()).await.expect("write request");
    let _ = send.finish();

    // Read the newline-terminated JSON response.
    let mut reader = BufReader::new(recv);
    let mut line = String::new();
    reader.read_line(&mut line).await.expect("read response");

    // Verify the response.
    let resp: serde_json::Value =
        serde_json::from_str(line.trim()).expect("parse JSON response");

    assert_eq!(resp["ok"], true, "expected ok=true, got: {resp}");
    assert_eq!(
        resp["data"]["username"],
        "alice",
        "expected username 'alice', got: {resp}"
    );

    // Graceful cleanup.
    client.shutdown().await;
    relay.shutdown().await.expect("relay shutdown");
}
