use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DatabaseConfig {
	pub url: String,
	pub test_url: String,
	pub max_connections: u32,
}

impl Default for DatabaseConfig {
	fn default() -> Self {
		Self {
			url: "postgres://crv:crv@127.0.0.1:5432/chronoverse_dev".to_owned(),
			test_url: "postgres://crv:crv@127.0.0.1:5432/chronoverse_test".to_owned(),
			max_connections: 10,
		}
	}
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RawDatabaseConfig {
	pub(super) url: Option<String>,
	pub(super) test_url: Option<String>,
	pub(super) max_connections: Option<u32>,
}
