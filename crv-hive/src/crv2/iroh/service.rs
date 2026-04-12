//! iroh control plane for crv-hive.
//!
//! Binary blob transfer is delegated to `iroh-blobs` on its own ALPN.
//! This module only handles control-plane RPCs such as user operations and
//! issuing `BlobTicket`s that clients can hand to `iroh-blobs` downloaders.
//!
//! ## Control protocol
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

use std::{future, sync::Arc};

use iroh::{
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler, Router},
};
use iroh_blobs::BlobsProtocol;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::crv2::ChronoverseApp;

use super::{
    controller::{self, HiveRequest, HiveResponse},
    iroh_client::IrohClient,
};

// ── RPC protocol handler ──────────────────────────────────────────────────────

#[derive(Clone)]
struct RpcProtocol {
    app: Arc<ChronoverseApp>,
}

impl std::fmt::Debug for RpcProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcProtocol").finish()
    }
}

impl RpcProtocol {
    fn new(app: Arc<ChronoverseApp>) -> Self {
        Self { app }
    }
}

impl ProtocolHandler for RpcProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        handle_connection(connection, Arc::clone(&self.app))
            .await
            .map_err(AcceptError::from_err)
    }
}

// ── IrohService ───────────────────────────────────────────────────────────────

/// Runs the iroh endpoint in server mode, accepting connections and dispatching
/// RPC calls to [`ChronoverseApp`].
pub struct IrohService {
    router: Router,
}

impl IrohService {
    /// Create a new service from an already-started [`IrohClient`] and the
    /// shared application state.
    pub fn new(client: IrohClient, app: Arc<ChronoverseApp>) -> Self {
        let rpc = RpcProtocol::new(Arc::clone(&app));
        let blobs = BlobsProtocol::new(app.cas_store().inner(), None);
        let router = Router::builder(client.endpoint().clone())
            .accept(super::iroh_client::ALPN, rpc)
            .accept(iroh_blobs::ALPN, blobs)
            .spawn();

        Self { router }
    }

    /// Keep the service alive until cancelled by the caller.
    pub async fn serve(self) {
        let _router = self.router;
        future::pending::<()>().await;
    }

    /// Gracefully close the underlying router and endpoint.
    pub async fn shutdown(self) -> Result<(), String> {
        self.router
            .shutdown()
            .await
            .map_err(|err| err.to_string())
    }
}

// ── Connection handler ────────────────────────────────────────────────────────

/// Accept bi-directional streams from one connection in a loop.
async fn handle_connection(conn: Connection, app: Arc<ChronoverseApp>) -> Result<(), std::io::Error> {
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

	Ok(())
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
        Ok(req) => controller::dispatch(&app, req).await,
    };

    let mut resp_bytes = match serde_json::to_vec(&response) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("failed to serialize response: {e}");
            return;
        }
    };
    resp_bytes.push(b'\n');

    let _ = send.write_all(&resp_bytes).await;
    let _ = send.finish();
}
