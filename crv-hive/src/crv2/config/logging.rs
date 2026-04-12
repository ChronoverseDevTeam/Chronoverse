use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct LoggingConfig {
	pub rust_log: String,
}

impl Default for LoggingConfig {
	fn default() -> Self {
		Self {
			rust_log: "info".to_owned(),
		}
	}
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RawLoggingConfig {
	pub(super) rust_log: Option<String>,
}
