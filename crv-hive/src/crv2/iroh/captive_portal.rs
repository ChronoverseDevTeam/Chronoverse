//! Captive-portal probe responder + node-ticket discovery API.
//!
//! This HTTP server running on port 80 serves two purposes:
//!
//! 1. **Captive-portal probe** (`GET /generate_204`) — iroh checks this on
//!    startup; we reply with the correct 204 response so the probe succeeds
//!    immediately instead of timing out.
//!
//! 2. **Node-ticket API** (`GET /crv/node-ticket`) — returns a JSON object
//!    containing the hive's `EndpointTicket` string so edge clients can
//!    discover the server's iroh address before connecting.
//!
//!    ```json
//!    { "ticket": "endpoint<base32...>" }
//!    ```
//!
//!    The ticket is optional at startup and can be set later via
//!    [`CaptivePortalServer::set_ticket`] once the iroh endpoint is ready.
//!    Until it is set the endpoint returns `503 Service Unavailable`.

use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    task::JoinHandle,
};

/// Shared, runtime-updatable ticket string.
type SharedTicket = Arc<RwLock<Option<String>>>;

/// A lightweight HTTP/1.1 server that answers iroh's captive-portal probe
/// and exposes a node-ticket discovery endpoint.
pub struct CaptivePortalServer {
    addr: SocketAddr,
    ticket: SharedTicket,
    handle: JoinHandle<()>,
}

impl CaptivePortalServer {
    /// Bind and start the server on `bind_addr`.
    ///
    /// Use `"0.0.0.0:80"` for production or `"0.0.0.0:8080"` when running
    /// without elevated privileges (adjust the relay URL accordingly).
    pub async fn start(bind_addr: SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind_addr).await?;
        let addr = listener.local_addr()?;
        let ticket: SharedTicket = Arc::new(RwLock::new(None));
        let ticket_clone = Arc::clone(&ticket);

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        let t = Arc::clone(&ticket_clone);
                        tokio::spawn(handle_connection(socket, t));
                    }
                    Err(err) => {
                        tracing::warn!("captive-portal accept error: {err}");
                        break;
                    }
                }
            }
        });

        Ok(Self { addr, ticket, handle })
    }

    /// The local address this server is bound to.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Publish the iroh node ticket so `GET /crv/node-ticket` can return it.
    ///
    /// Call this once the iroh endpoint is ready and its [`EndpointAddr`] is
    /// known. Subsequent calls overwrite the previous value.
    pub fn set_ticket(&self, ticket: impl Into<String>) {
        *self.ticket.write().expect("ticket lock poisoned") = Some(ticket.into());
    }

    /// Stop the captive-portal server.
    pub fn shutdown(self) {
        self.handle.abort();
    }
}

// ── HTTP connection handler ───────────────────────────────────────────────────

/// Parsed request line.
struct RequestLine {
    path: String,
    challenge: String,
}

/// Read HTTP/1.1 request line + headers; extract path and X-Iroh-Challenge.
async fn read_request(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>) -> Option<RequestLine> {
    let mut path = String::new();
    let mut challenge = String::new();
    let mut first = true;
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => return None,
            _ => {}
        }

        if line == "\r\n" || line == "\n" {
            break;
        }

        if first {
            // e.g. "GET /generate_204 HTTP/1.1"
            if let Some(p) = line.split_whitespace().nth(1) {
                path = p.to_string();
            }
            first = false;
        } else {
            let lower = line.to_lowercase();
            if lower.starts_with("x-iroh-challenge:") {
                if let Some(pos) = lower.find(':') {
                    challenge = line[pos + 1..].trim().to_string();
                }
            }
        }
    }

    Some(RequestLine { path, challenge })
}

async fn handle_connection(socket: tokio::net::TcpStream, ticket: SharedTicket) {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);

    let Some(req) = read_request(&mut reader).await else { return };

    let response = if req.path == "/generate_204" {
        // ── Captive-portal probe ──────────────────────────────────────────
        let body = format!("204 response to {}", req.challenge);
        format!(
            "HTTP/1.1 204 No Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    } else if req.path == "/crv/node-ticket" {
        // ── Node-ticket discovery API ─────────────────────────────────────
        match ticket.read().expect("ticket lock poisoned").as_deref() {
            Some(t) => {
                let body = format!("{{\"ticket\":\"{t}\"}}");
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            }
            None => {
                let body = "{\"error\":\"node not ready yet\"}";
                format!(
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
            }
        }
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
    };

    let _ = writer.write_all(response.as_bytes()).await;
}
