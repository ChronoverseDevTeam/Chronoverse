//! iroh endpoint for crv-hive.
//!
//! Wraps an [`iroh::Endpoint`] that connects through the embedded relay server
//! to enable NAT traversal between the hive and edge clients.
//!
//! # Quick start
//!
//! ```ignore
//! # tokio_test::block_on(async {
//! use crv_hive::crv2::iroh::iroh_client::{IrohClient, ALPN};
//! use crv_hive::crv2::config::IrohConfig;
//!
//! let config = IrohConfig::default();
//! let client = IrohClient::start(&config).await.unwrap();
//!
//! println!("hive node id: {}", client.id());
//!
//! // Accept one incoming connection.
//! if let Some(conn) = client.accept().await {
//!     println!("peer connected: {}", conn.remote_id());
//! }
//!
//! client.shutdown().await;
//! # });
//! ```

use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMap, RelayMode, SecretKey,
    address_lookup::{PkarrPublisher, PkarrResolver},
    endpoint::Connection,
};
use iroh_tickets::endpoint::EndpointTicket;
use thiserror::Error;

use crate::crv2::config::IrohConfig;

// ── ALPN ─────────────────────────────────────────────────────────────────────

/// Application-Layer Protocol Negotiation identifier for the crv hive protocol.
pub const ALPN: &[u8] = b"crv/hive/1";

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors produced while starting or using the iroh endpoint.
#[derive(Debug, Error)]
pub enum IrohClientError {
    /// The relay URL could not be parsed.
    #[error("invalid relay URL: {0}")]
    RelayUrl(#[from] iroh::RelayUrlParseError),

    /// The Pkarr URL could not be parsed.
    #[error("invalid pkarr URL: {0}")]
    PkarrUrl(#[from] url::ParseError),

    /// The secret key hex string is invalid.
    #[error("invalid secret_key: expected 64 hex chars (32 bytes), got {0}")]
    SecretKey(String),

    /// The endpoint could not be bound.
    #[error("failed to bind iroh endpoint: {0}")]
    Bind(#[from] iroh::endpoint::BindError),

    /// A connection attempt failed.
    #[error("connection failed: {0}")]
    Connect(#[from] iroh::endpoint::ConnectError),
}

// ── IrohClient ────────────────────────────────────────────────────────────────

/// An iroh endpoint for the crv-hive service.
///
/// Dropping this value closes the endpoint immediately. Call
/// [`IrohClient::shutdown`] for a graceful close.
pub struct IrohClient {
    endpoint: Endpoint,
}

impl IrohClient {
    /// Start an iroh endpoint from the application [`IrohConfig`].
    pub async fn start(config: &IrohConfig) -> Result<Self, IrohClientError> {
        let relay_url: iroh::RelayUrl = config.relay_url.parse()?;
        let relay_mode = RelayMode::Custom(RelayMap::from_iter(std::iter::once(relay_url)));

        let secret_key = parse_hex_secret_key(&config.secret_key)?;

        let pkarr_url: url::Url = config.pkarr_url.parse()?;

        let endpoint = Endpoint::empty_builder()
            .relay_mode(relay_mode)
            .alpns(vec![ALPN.to_vec()])
            .secret_key(secret_key)
            .address_lookup(PkarrPublisher::builder(pkarr_url.clone()))
            .address_lookup(PkarrResolver::builder(pkarr_url))
            .bind()
            .await?;

        Ok(Self { endpoint })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    // ── Identity ─────────────────────────────────────────────────────────────

    /// Returns the [`EndpointId`] (public key) of this hive node.
    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Returns the full [`EndpointAddr`] (id + relay URL + direct addresses)
    /// of this hive node. Peers need this address to connect.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Returns a connection ticket encoding this node's [`EndpointAddr`].
    ///
    /// The ticket string starts with `endpoint` followed by base32 data.
    /// Share it with edge clients so they can parse it with
    /// `EndpointTicket::from_str` and connect.
    pub fn ticket(&self) -> EndpointTicket {
        EndpointTicket::from(self.endpoint.addr())
    }

    // ── Outbound ─────────────────────────────────────────────────────────────

    /// Establish a connection to a remote peer identified by `addr`.
    ///
    /// `alpn` selects the application protocol; use [`ALPN`] for the default
    /// crv hive protocol.
    pub async fn connect(
        &self,
        addr: EndpointAddr,
        alpn: &[u8],
    ) -> Result<Connection, IrohClientError> {
        Ok(self.endpoint.connect(addr, alpn).await?)
    }

    // ── Inbound ──────────────────────────────────────────────────────────────

    /// Wait for the next incoming connection.
    ///
    /// Returns `None` when the endpoint has been closed. The returned
    /// [`Connection`] is fully established and ALPN-verified before being
    /// returned.
    pub async fn accept(&self) -> Option<Connection> {
        let incoming = self.endpoint.accept().await?;
        match incoming.await {
            Ok(conn) => Some(conn),
            Err(err) => {
                tracing::warn!("failed to accept incoming connection: {err}");
                None
            }
        }
    }

    // ── Lifecycle ────────────────────────────────────────────────────────────

    /// Gracefully close the endpoint and wait for all tasks to finish.
    pub async fn shutdown(self) {
        self.endpoint.close().await;
    }
}

/// Parse a 64-character hex string into a 32-byte [`SecretKey`].
fn parse_hex_secret_key(hex: &str) -> Result<SecretKey, IrohClientError> {
    let hex = hex.trim();
    if hex.len() != 64 {
        return Err(IrohClientError::SecretKey(format!(
            "length {} (expected 64)",
            hex.len()
        )));
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0]).ok_or_else(|| {
            IrohClientError::SecretKey(format!("invalid hex char '{}'", chunk[0] as char))
        })?;
        let lo = hex_nibble(chunk[1]).ok_or_else(|| {
            IrohClientError::SecretKey(format!("invalid hex char '{}'", chunk[1] as char))
        })?;
        bytes[i] = (hi << 4) | lo;
    }
    Ok(SecretKey::from_bytes(&bytes))
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
