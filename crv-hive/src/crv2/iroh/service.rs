//! iroh service layer: accepts connections and dispatches RPC calls to
//! [`ChronoverseApp`].
//!
//! ## Wire protocol
//!
//! Every RPC is a single **bi-directional stream**:
//!
//! ```text
//! client ──► server   (newline-terminated JSON request)
//! client ◄── server   (newline-terminated JSON response)
//! ```
//!
//! ### Request
//!
//! ```json
//! {"method":"register_user","username":"alice","password":"secret"}
//! ```
//!
//! ### Response (success)
//!
//! ```json
//! {"ok":true,"data":{"username":"alice"}}
//! ```
//!
//! ### Response (error)
//!
//! ```json
//! {"ok":false,"error":"<reason>"}
//! ```

use std::sync::Arc;

use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::crv2::{ChronoverseApp, RegisterUserReq};

use super::iroh_client::IrohClient;

// ── Wire types ────────────────────────────────────────────────────────────────

/// Incoming RPC request (deserialized from JSON).
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
enum HiveRequest {
    RegisterUser { username: String, password: String },
}

/// Outgoing RPC response (serialized to JSON).
#[derive(Serialize)]
struct HiveResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl HiveResponse {
    fn ok(data: serde_json::Value) -> Self {
        Self { ok: true, data: Some(data), error: None }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, data: None, error: Some(msg.into()) }
    }
}

// ── IrohService ───────────────────────────────────────────────────────────────

/// Runs the iroh endpoint in server mode, accepting connections and dispatching
/// RPC calls to [`ChronoverseApp`].
pub struct IrohService {
    client: IrohClient,
    app: Arc<ChronoverseApp>,
}

impl IrohService {
    /// Create a new service from an already-started [`IrohClient`] and the
    /// shared application state.
    pub fn new(client: IrohClient, app: Arc<ChronoverseApp>) -> Self {
        Self { client, app }
    }

    /// Run the accept loop until the endpoint is closed or an irrecoverable
    /// error occurs.  Spawn tasks for each connection so this never blocks.
    pub async fn serve(self) {
        loop {
            match self.client.accept().await {
                Some(conn) => {
                    let app = Arc::clone(&self.app);
                    tokio::spawn(async move {
                        handle_connection(conn, app).await;
                    });
                }
                None => {
                    tracing::info!("iroh endpoint closed, stopping service");
                    break;
                }
            }
        }
    }

    /// Gracefully close the underlying iroh endpoint.
    pub async fn shutdown(self) {
        self.client.shutdown().await;
    }
}

// ── Connection handler ────────────────────────────────────────────────────────

/// Accept bi-directional streams from one connection in a loop.
async fn handle_connection(conn: Connection, app: Arc<ChronoverseApp>) {
    let peer = conn.remote_id();
    tracing::info!("iroh: new connection from {peer}");

    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                let app = Arc::clone(&app);
                tokio::spawn(async move {
                    handle_stream(send, recv, app).await;
                });
            }
            Err(err) => {
                tracing::debug!("iroh: connection from {peer} closed: {err}");
                break;
            }
        }
    }
}

// ── Stream handler ────────────────────────────────────────────────────────────

/// Read one JSON request, dispatch it, and write the JSON response.
async fn handle_stream(
    mut send: SendStream,
    recv: RecvStream,
    app: Arc<ChronoverseApp>,
) {
    let mut reader = BufReader::new(recv);
    let mut line = String::new();

    if reader.read_line(&mut line).await.is_err() || line.trim().is_empty() {
        return;
    }

    let response = match serde_json::from_str::<HiveRequest>(line.trim()) {
        Err(e) => HiveResponse::err(format!("invalid request: {e}")),
        Ok(req) => dispatch(&app, req),
    };

    let mut resp_bytes = match serde_json::to_vec(&response) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("failed to serialize response: {e}");
            return;
        }
    };
    resp_bytes.push(b'\n');

    let _ = send.write_all(&resp_bytes).await;
    let _ = send.finish();
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

fn dispatch(app: &ChronoverseApp, req: HiveRequest) -> HiveResponse {
    match req {
        HiveRequest::RegisterUser { username, password } => {
            match app.register_user(&RegisterUserReq { username, password }) {
                Ok(rsp) => HiveResponse::ok(json!({"username": rsp.username})),
                Err(e) => HiveResponse::err(e),
            }
        }
    }
}
