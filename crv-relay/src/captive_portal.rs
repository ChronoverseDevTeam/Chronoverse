//! Combined captive-portal probe responder and private Pkarr relay server.
//!
//! This HTTP server serves two purposes:
//!
//! 1. **Captive-portal probe** (`GET /generate_204`) — iroh checks this on
//!    startup; we reply with the correct 204 response so the probe succeeds
//!    immediately instead of timing out.
//!
//! 2. **Private Pkarr relay** (`PUT /{z32key}` / `GET /{z32key}`) — iroh's
//!    [`PkarrPublisher`] and [`PkarrResolver`] use HTTP to publish and resolve
//!    endpoint addressing information.  Clients configure the base URL of this
//!    server as their Pkarr relay URL, and iroh will append the z32-encoded
//!    node ID to form the full path.
//!
//! Running both on the same port avoids opening an extra listener socket.
//!
//! [`PkarrPublisher`]: iroh::address_lookup::PkarrPublisher
//! [`PkarrResolver`]: iroh::address_lookup::PkarrResolver

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::RwLock,
    task::JoinHandle,
};

/// In-memory store for Pkarr signed packets, keyed by z32-encoded public key.
type PkarrStore = Arc<RwLock<HashMap<String, Vec<u8>>>>;

/// Shared state passed to every connection handler.
struct ServerState {
    pkarr: PkarrStore,
    /// The most recently published hive `EndpointTicket` string (if any).
    hive_ticket: RwLock<Option<String>>,
}

/// A lightweight HTTP/1.1 server that answers iroh's captive-portal probe
/// and doubles as a private Pkarr relay for endpoint-address publication.
pub struct CaptivePortalServer {
    addr: SocketAddr,
    handle: JoinHandle<()>,
}

impl CaptivePortalServer {
    /// Bind and start the server on `bind_addr`.
    ///
    /// Use `"0.0.0.0:80"` for production or `"0.0.0.0:8080"` when running
    /// without elevated privileges.
    pub async fn start(bind_addr: SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind_addr).await?;
        let addr = listener.local_addr()?;
        let state = Arc::new(ServerState {
            pkarr: Arc::new(RwLock::new(HashMap::new())),
            hive_ticket: RwLock::new(None),
        });

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, _)) => {
                        let s = Arc::clone(&state);
                        tokio::spawn(handle_connection(socket, s));
                    }
                    Err(err) => {
                        tracing::warn!("captive-portal accept error: {err}");
                        break;
                    }
                }
            }
        });

        Ok(Self { addr, handle })
    }

    /// The local address this server is bound to.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The base Pkarr relay URL that iroh clients should configure.
    ///
    /// Pass this to `PkarrPublisher::builder(url)` and `PkarrResolver::builder(url)`.
    pub fn pkarr_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Stop the captive-portal / Pkarr relay server.
    pub fn shutdown(self) {
        self.handle.abort();
    }
}

// ── HTTP request parser ───────────────────────────────────────────────────────

/// Parsed HTTP/1.1 request headers.
struct ParsedRequest {
    method: String,
    path: String,
    challenge: String,
    content_length: usize,
}

/// Read request line + headers; return parsed metadata (no body).
async fn parse_headers(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Option<ParsedRequest> {
    let mut method = String::new();
    let mut path = String::new();
    let mut challenge = String::new();
    let mut content_length: usize = 0;
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
            // e.g. "PUT /o3dks...6uyy HTTP/1.1"
            let mut parts = line.split_whitespace();
            method = parts.next().unwrap_or("").to_string();
            path = parts.next().unwrap_or("").to_string();
            first = false;
        } else {
            let lower = line.to_lowercase();
            if lower.starts_with("content-length:") {
                if let Some(pos) = lower.find(':') {
                    content_length = line[pos + 1..].trim().parse().unwrap_or(0);
                }
            } else if lower.starts_with("x-iroh-challenge:") {
                if let Some(pos) = lower.find(':') {
                    challenge = line[pos + 1..].trim().to_string();
                }
            }
        }
    }

    Some(ParsedRequest { method, path, challenge, content_length })
}

// ── Connection handler ────────────────────────────────────────────────────────

async fn handle_connection(socket: tokio::net::TcpStream, state: Arc<ServerState>) {
    let (reader, mut writer) = socket.into_split();
    let mut reader = BufReader::new(reader);

    let Some(req) = parse_headers(&mut reader).await else { return };

    let response: Vec<u8> = match req.path.as_str() {
        // ── Captive-portal probe ──────────────────────────────────────────────
        "/generate_204" => {
            let body = format!("204 response to {}", req.challenge);
            format!(
                "HTTP/1.1 204 No Content\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .into_bytes()
        }

        // ── Hive ticket: PUT /hive_ticket ─────────────────────────────────────
        "/hive_ticket" if req.method == "PUT" => {
            if req.content_length == 0 {
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec()
            } else {
                let mut body = vec![0u8; req.content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }
                let ticket = String::from_utf8_lossy(&body).to_string();
                *state.hive_ticket.write().await = Some(ticket);
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            }
        }

        // ── Hive ticket: GET /hive_ticket ─────────────────────────────────────
        "/hive_ticket" if req.method == "GET" => {
            match state.hive_ticket.read().await.as_deref() {
                Some(ticket) => {
                    let body = ticket.as_bytes();
                    let mut resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    resp.extend_from_slice(body);
                    resp
                }
                None => {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                }
            }
        }

        // ── Pkarr relay: PUT /{z32key} ────────────────────────────────────────
        key if req.method == "PUT" => {
            let key = key.trim_start_matches('/').to_string();
            if req.content_length == 0 {
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_vec()
            } else {
                // Read the binary signed-packet payload.
                let mut body = vec![0u8; req.content_length];
                if reader.read_exact(&mut body).await.is_err() {
                    return;
                }
                state.pkarr.write().await.insert(key, body);
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            }
        }

        // ── Pkarr relay: GET /{z32key} ────────────────────────────────────────
        key if req.method == "GET" => {
            let key = key.trim_start_matches('/').to_string();
            match state.pkarr.read().await.get(&key).cloned() {
                Some(payload) => {
                    let mut resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    )
                    .into_bytes();
                    resp.extend_from_slice(&payload);
                    resp
                }
                None => {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_vec()
                }
            }
        }

        // ── Anything else ─────────────────────────────────────────────────────
        _ => {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        }
    };

    let _ = writer.write_all(&response).await;
}
