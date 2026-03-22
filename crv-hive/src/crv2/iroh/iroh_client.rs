//! iroh endpoint for crv-hive.
//!
//! Wraps an [`iroh::Endpoint`] that connects through the embedded relay server
//! to enable NAT traversal between the hive and edge clients.
//!
//! # Quick start
//!
//! ```no_run
//! # tokio_test::block_on(async {
//! use crv_hive::crv2::iroh::iroh_client::{IrohClient, IrohClientConfig, ALPN};
//!
//! // Start a server-side endpoint backed by the local relay.
//! let client = IrohClient::start(IrohClientConfig {
//!     relay_url: Some("http://127.0.0.1:3340".parse().unwrap()),
//!     secret_key: None,
//! })
//! .await
//! .unwrap();
//!
//! println!("hive node id: {}", client.id());
//!
//! // Accept one incoming connection.
//! if let Some(conn) = client.accept().await {
//!     println!("peer connected: {}", conn.remote_address());
//! }
//!
//! client.shutdown().await;
//! # });
//! ```

use iroh::{
    Endpoint, EndpointAddr, EndpointId, RelayMap, RelayMode, RelayUrl, SecretKey,
    endpoint::Connection,
};
use iroh_tickets::endpoint::EndpointTicket;
use thiserror::Error;

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

    /// The endpoint could not be bound.
    #[error("failed to bind iroh endpoint: {0}")]
    Bind(#[from] iroh::endpoint::BindError),

    /// A connection attempt failed.
    #[error("connection failed: {0}")]
    Connect(#[from] iroh::endpoint::ConnectError),
}

// ── Config ────────────────────────────────────────────────────────────────────

/// Configuration for [`IrohClient`].
pub struct IrohClientConfig {
    /// URL of the relay server to use.
    ///
    /// When `Some`, only that relay is used (`RelayMode::Custom`). When `None`,
    /// the n0 default relay servers are used (`RelayMode::Default`).
    pub relay_url: Option<RelayUrl>,

    /// Secret key for this endpoint. A fresh random key is generated when `None`.
    pub secret_key: Option<SecretKey>,
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
    /// Start an iroh endpoint with the given configuration.
    pub async fn start(config: IrohClientConfig) -> Result<Self, IrohClientError> {
        let relay_mode = match config.relay_url {
            Some(url) => {
                let map = RelayMap::from_iter(std::iter::once(url));
                RelayMode::Custom(map)
            }
            None => RelayMode::Default,
        };

        // Use empty_builder (no PkarrPublisher, no DnsAddressLookup) to avoid
        // outbound requests to public DNS/relay infrastructure. This is correct
        // for private deployments that run their own relay.
        let mut builder = Endpoint::empty_builder()
            .relay_mode(relay_mode)
            .alpns(vec![ALPN.to_vec()]);

        if let Some(key) = config.secret_key {
            builder = builder.secret_key(key);
        }

        let endpoint = builder.bind().await?;
        Ok(Self { endpoint })
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
