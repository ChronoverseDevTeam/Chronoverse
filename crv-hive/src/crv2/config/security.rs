use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct SecurityConfig {
	pub jwt_secret: String,
}

impl Default for SecurityConfig {
	fn default() -> Self {
		Self {
			jwt_secret: "dev-secret".to_owned(),
		}
	}
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct RawSecurityConfig {
	pub(super) jwt_secret: Option<String>,
}
