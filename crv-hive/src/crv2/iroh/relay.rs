//! Embedded iroh relay server for crv-hive.
//!
//! Supports three modes:
//! - **Plain HTTP** – relay only, no TLS.
//! - **HTTPS** – relay served over TLS (manual cert/key DER or auto self-signed
//!   via [`TlsCert::SelfSigned`]).
//! - **HTTPS + QUIC** – same as above, plus a QUIC address-discovery server on
//!   a separate address.
//!
//! # Quick start
//!
//! ```ignore
//! # tokio_test::block_on(async {
//! use std::net::SocketAddr;
//! use crv_hive::crv2::iroh::relay::{RelayServer, RelayServerConfig, RelayTlsConfig, TlsCert};
//!
//! // Plain HTTP:
//! let srv = RelayServer::start("0.0.0.0:3340".parse().unwrap()).await.unwrap();
//! println!("http relay on {:?}", srv.http_addr());
//! srv.shutdown().await.unwrap();
//!
//! // HTTPS + QUIC with a self-signed cert:
//! let srv = RelayServer::start_with_config(RelayServerConfig {
//!     http_bind_addr:  "0.0.0.0:3340".parse().unwrap(),
//!     tls: Some(RelayTlsConfig {
//!         https_bind_addr: "0.0.0.0:3341".parse().unwrap(),
//!         cert: TlsCert::SelfSigned {
//!             subject_alt_names: vec!["relay.example.com".to_string()],
//!         },
//!     }),
//!     quic_bind_addr: Some("0.0.0.0:7842".parse().unwrap()),
//! }).await.unwrap();
//! println!("https on {:?}, quic on {:?}", srv.https_addr(), srv.quic_addr());
//! srv.shutdown().await.unwrap();
//! # });
//! ```

use std::{net::SocketAddr, sync::Arc};

use iroh_relay::server::{
    AccessConfig, CertConfig, Limits, QuicConfig, RelayConfig, Server,
    ServerConfig, SupervisorError, TlsConfig,
};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use thiserror::Error;

// ── Public error type ────────────────────────────────────────────────────────

/// Errors produced while starting the relay server.
#[derive(Debug, Error)]
pub enum RelayStartError {
    /// QUIC was requested but TLS is not configured (QUIC requires TLS).
    #[error("QUIC requires TLS to be configured")]
    QuicRequiresTls,
    /// Self-signed certificate generation failed.
    #[error("certificate generation failed: {0}")]
    CertGen(#[from] rcgen::Error),
    /// Building the rustls ServerConfig failed.
    #[error("TLS configuration error: {0}")]
    Tls(#[from] rustls::Error),
    /// The iroh relay server could not be spawned.
    #[error("failed to spawn relay server: {0}")]
    Spawn(#[from] iroh_relay::server::SpawnError),
}

// ── Configuration types ──────────────────────────────────────────────────────

/// Source of the TLS certificate used by the relay.
pub enum TlsCert {
    /// Generate a fresh self-signed certificate for the given Subject Alt Names.
    ///
    /// Uses [`rcgen`] under the hood. Suitable for development and internal
    /// deployments. In production, prefer [`TlsCert::Manual`] with a cert
    /// issued by a trusted CA.
    SelfSigned {
        /// Subject Alternative Names embedded in the certificate
        /// (e.g. `["relay.example.com", "127.0.0.1"]`).
        subject_alt_names: Vec<String>,
    },

    /// Use an existing certificate chain and private key supplied as raw DER
    /// bytes.
    ///
    /// `cert_chain` is an ordered list of DER-encoded `CertificateDer` buffers
    /// (leaf certificate first). `private_key_der` is a PKCS#8 DER-encoded
    /// private key.
    Manual {
        /// Ordered DER certificate chain (leaf first).
        cert_chain: Vec<Vec<u8>>,
        /// PKCS#8-encoded private key DER bytes.
        private_key_der: Vec<u8>,
    },
}

/// TLS settings for the HTTPS relay endpoint.
pub struct RelayTlsConfig {
    /// Socket address on which the HTTPS server will bind.
    ///
    /// Port `443` is the conventional choice for public relays.
    pub https_bind_addr: SocketAddr,

    /// Where the certificate and private key come from.
    pub cert: TlsCert,
}

/// Full configuration for a [`RelayServer`].
pub struct RelayServerConfig {
    /// Socket address for the plain-HTTP listener.
    ///
    /// When TLS is enabled this socket serves the captive-portal
    /// (`/generate_204`) endpoint on plain HTTP.  When TLS is disabled this
    /// socket serves the relay endpoint directly.
    pub http_bind_addr: SocketAddr,

    /// Optional TLS configuration.  When `None` the relay runs over plain HTTP.
    pub tls: Option<RelayTlsConfig>,

    /// Optional bind address for the QUIC address-discovery (QAD) server.
    ///
    /// When `Some`, a QUIC server is also started at this address. QUIC always
    /// requires TLS, so [`RelayServerConfig::tls`] must be `Some` too or
    /// [`RelayStartError::QuicRequiresTls`] is returned.
    ///
    /// The conventional QUIC port used by iroh is `7842`.
    pub quic_bind_addr: Option<SocketAddr>,
}

// ── RelayServer ───────────────────────────────────────────────────────────────

/// A running iroh relay server.
///
/// Dropping this value aborts all background tasks immediately.  Call
/// [`RelayServer::shutdown`] for a graceful stop.
pub struct RelayServer {
    inner: Server,
}

impl RelayServer {
    /// Start a plain-HTTP relay server bound to `addr`.
    ///
    /// Equivalent to `start_with_config` with `tls = None` and
    /// `quic_bind_addr = None`.
    pub async fn start(addr: SocketAddr) -> Result<Self, RelayStartError> {
        Self::start_with_config(RelayServerConfig {
            http_bind_addr: addr,
            tls: None,
            quic_bind_addr: None,
        })
        .await
    }

    /// Start a relay server with full control over TLS and QUIC.
    pub async fn start_with_config(config: RelayServerConfig) -> Result<Self, RelayStartError> {
        if config.quic_bind_addr.is_some() && config.tls.is_none() {
            return Err(RelayStartError::QuicRequiresTls);
        }

        let (iroh_tls, quic_config) = match config.tls {
            None => (None, None),
            Some(tls_cfg) => {
                // Resolve the certificate source into raw DER byte buffers so
                // we can build two independent `rustls::ServerConfig` instances
                // (one for HTTPS, one for QUIC) without needing Clone.
                let (chain_ders, key_der_bytes) = resolve_cert(tls_cfg.cert)?;

                // Re-usable rustls ServerConfig factory.
                let build_rustls = |chain: &[Vec<u8>], key: &[u8]| {
                    let cert_chain: Vec<CertificateDer<'static>> =
                        chain.iter().map(|b| CertificateDer::from(b.clone())).collect();
                    let private_key =
                        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.to_vec()));
                    rustls::ServerConfig::builder_with_provider(Arc::new(
                        rustls::crypto::aws_lc_rs::default_provider(),
                    ))
                    .with_safe_default_protocol_versions()?
                    .with_no_client_auth()
                    .with_single_cert(cert_chain, private_key)
                };

                // Build the QUIC ServerConfig first (if requested).
                let quic = config
                    .quic_bind_addr
                    .map(|addr| -> Result<QuicConfig, RelayStartError> {
                        Ok(QuicConfig {
                            bind_addr: addr,
                            server_config: build_rustls(&chain_ders, &key_der_bytes)?,
                        })
                    })
                    .transpose()?;

                // Build the HTTPS ServerConfig.
                let https_server_config = build_rustls(&chain_ders, &key_der_bytes)?;

                // The public cert chain exposed through `Server::certificates()`.
                let exposed_certs: Vec<CertificateDer<'static>> =
                    chain_ders.iter().map(|b| CertificateDer::from(b.clone())).collect();

                let tls: TlsConfig<(), ()> = TlsConfig {
                    https_bind_addr: tls_cfg.https_bind_addr,
                    // Advertise the QUIC address when QAD is enabled.
                    quic_bind_addr: config
                        .quic_bind_addr
                        .unwrap_or(tls_cfg.https_bind_addr),
                    cert: CertConfig::Manual { certs: exposed_certs },
                    server_config: https_server_config,
                };

                (Some(tls), quic)
            }
        };

        let iroh_config = ServerConfig::<(), ()> {
            relay: Some(RelayConfig {
                http_bind_addr: config.http_bind_addr,
                tls: iroh_tls,
                limits: Limits::default(),
                key_cache_capacity: None,
                access: AccessConfig::Everyone,
            }),
            quic: quic_config,
            // `server` feature always enables this field.
            metrics_addr: None,
        };

        let server = Server::spawn(iroh_config).await?;
        Ok(Self { inner: server })
    }

    // ── Address accessors ────────────────────────────────────────────────────

    /// The HTTP server address (plain HTTP or captive-portal endpoint when TLS
    /// is active).
    pub fn http_addr(&self) -> Option<SocketAddr> {
        self.inner.http_addr()
    }

    /// The HTTPS server address.  `None` when TLS was not configured.
    pub fn https_addr(&self) -> Option<SocketAddr> {
        self.inner.https_addr()
    }

    /// The QUIC address-discovery server address.  `None` when QUIC was not
    /// configured.
    pub fn quic_addr(&self) -> Option<SocketAddr> {
        self.inner.quic_addr()
    }

    /// The DER-encoded certificate chain, if the server was configured with
    /// [`TlsCert::Manual`] or [`TlsCert::SelfSigned`].
    pub fn certificates(&self) -> Option<Vec<CertificateDer<'static>>> {
        self.inner.certificates()
    }

    // ── Lifecycle ───────────────────────────────────────────────────────────

    /// Gracefully shut down the relay server and wait for all tasks to finish.
    pub async fn shutdown(self) -> Result<(), SupervisorError> {
        self.inner.shutdown().await
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Resolve a [`TlsCert`] into `(cert_chain_ders, private_key_pkcs8_der)`.
fn resolve_cert(cert: TlsCert) -> Result<(Vec<Vec<u8>>, Vec<u8>), RelayStartError> {
    match cert {
        TlsCert::SelfSigned { subject_alt_names } => {
            let CertifiedKey { cert, signing_key } =
                generate_simple_self_signed(subject_alt_names)?;
            Ok((vec![cert.der().to_vec()], signing_key.serialize_der()))
        }
        TlsCert::Manual {
            cert_chain,
            private_key_der,
        } => Ok((cert_chain, private_key_der)),
    }
}
