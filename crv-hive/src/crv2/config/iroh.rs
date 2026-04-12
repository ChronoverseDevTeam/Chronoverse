use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct IrohConfig {
	pub relay_url: String,
	/// Base URL of the private Pkarr relay (the captive-portal server).
	/// iroh will publish/resolve node addresses against this URL.
	pub pkarr_url: String,
	/// Hex-encoded ed25519 secret key (64 hex chars = 32 bytes).
	/// A random key is generated automatically when the default config is created.
	/// Changing this value changes the node's identity (NodeId / PublicKey).
	pub secret_key: String,
}

impl Default for IrohConfig {
	fn default() -> Self {
		use rand::RngCore;
		let mut bytes = [0u8; 32];
		rand::rngs::OsRng.fill_bytes(&mut bytes);
		let secret_key = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();

		Self {
			relay_url: "http://127.0.0.1:3340".to_owned(),
			pkarr_url: "http://127.0.0.1:80".to_owned(),
			secret_key,
		}
	}
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RawIrohConfig {
	pub(super) relay_url: Option<String>,
	pub(super) pkarr_url: Option<String>,
	pub(super) secret_key: Option<String>,
}
