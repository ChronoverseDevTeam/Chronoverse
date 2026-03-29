use std::{
	ffi::OsString,
	fs,
	path::{Path, PathBuf},
};

use once_cell::sync::OnceCell;
use serde::Deserialize;
use thiserror::Error;

pub const DEFAULT_CONFIG_PATH: &str = "hive.toml";

static CONFIG: OnceCell<AppConfig> = OnceCell::new();

#[derive(Debug, Clone)]
pub enum ConfigSource {
	Defaults,
	File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
	pub config: AppConfig,
	pub source: ConfigSource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
	pub database: DatabaseConfig,
	pub logging: LoggingConfig,
	pub iroh: IrohConfig,
	pub service: ServiceConfig,
	pub storage: StorageConfig,
	pub security: SecurityConfig,
}

impl Default for AppConfig {
	fn default() -> Self {
		Self {
			database: DatabaseConfig::default(),
			logging: LoggingConfig::default(),
			iroh: IrohConfig::default(),
			service: ServiceConfig::default(),
			storage: StorageConfig::default(),
			security: SecurityConfig::default(),
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
	pub url: String,
	pub test_url: String,
	pub max_connections: u32,
}

impl Default for DatabaseConfig {
	fn default() -> Self {
		Self {
			url: "postgres://crv:crv@127.0.0.1:55432/chronoverse_dev".to_owned(),
			test_url: "postgres://crv:crv@127.0.0.1:55432/chronoverse_test".to_owned(),
			max_connections: 10,
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IrohConfig {
	pub relay_bind_addr: String,
	pub relay_url: String,
	pub captive_portal_addr: String,
}

impl Default for IrohConfig {
	fn default() -> Self {
		Self {
			relay_bind_addr: "0.0.0.0:3340".to_owned(),
			relay_url: "http://127.0.0.1:3340".to_owned(),
			captive_portal_addr: "0.0.0.0:80".to_owned(),
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServiceConfig {
	pub hive_address: String,
}

impl Default for ServiceConfig {
	fn default() -> Self {
		Self {
			hive_address: "0.0.0.0:34560".to_owned(),
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
	pub repository_path: PathBuf,
	pub upload_cache_path: PathBuf,
}

impl Default for StorageConfig {
	fn default() -> Self {
		Self {
			repository_path: PathBuf::from("./data/shards"),
			upload_cache_path: PathBuf::from("./data/upload_cache"),
		}
	}
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Error)]
pub enum ConfigError {
	#[error("missing config path after -c/--config")]
	MissingConfigPath,
	#[error("unsupported argument: {0}")]
	UnsupportedArgument(String),
	#[error("failed to read config file {path}: {source}")]
	ReadConfig {
		path: PathBuf,
		#[source]
		source: std::io::Error,
	},
	#[error("failed to parse config file {path}: {source}")]
	ParseConfig {
		path: PathBuf,
		#[source]
		source: toml::de::Error,
	},
	#[error("application config has already been initialized")]
	AlreadyInitialized,
}

#[derive(Debug, Default, Deserialize)]
struct RawAppConfig {
	#[serde(default)]
	database: Option<RawDatabaseConfig>,
	#[serde(default)]
	logging: Option<RawLoggingConfig>,
	#[serde(default)]
	iroh: Option<RawIrohConfig>,
	#[serde(default)]
	service: Option<RawServiceConfig>,
	#[serde(default)]
	storage: Option<RawStorageConfig>,
	#[serde(default)]
	security: Option<RawSecurityConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDatabaseConfig {
	url: Option<String>,
	test_url: Option<String>,
	max_connections: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLoggingConfig {
	rust_log: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawIrohConfig {
	relay_bind_addr: Option<String>,
	relay_url: Option<String>,
	captive_portal_addr: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawServiceConfig {
	hive_address: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawStorageConfig {
	repository_path: Option<PathBuf>,
	upload_cache_path: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
struct RawSecurityConfig {
	jwt_secret: Option<String>,
}

impl RawAppConfig {
	fn into_config(self) -> AppConfig {
		let mut config = AppConfig::default();

		if let Some(database) = self.database {
			if let Some(url) = database.url {
				config.database.url = url;
			}
			if let Some(test_url) = database.test_url {
				config.database.test_url = test_url;
			}
			if let Some(max_connections) = database.max_connections {
				config.database.max_connections = max_connections;
			}
		}

		if let Some(logging) = self.logging {
			if let Some(rust_log) = logging.rust_log {
				config.logging.rust_log = rust_log;
			}
		}

		if let Some(iroh) = self.iroh {
			if let Some(relay_bind_addr) = iroh.relay_bind_addr {
				config.iroh.relay_bind_addr = relay_bind_addr;
			}
			if let Some(relay_url) = iroh.relay_url {
				config.iroh.relay_url = relay_url;
			}
			if let Some(captive_portal_addr) = iroh.captive_portal_addr {
				config.iroh.captive_portal_addr = captive_portal_addr;
			}
		}

		if let Some(service) = self.service {
			if let Some(hive_address) = service.hive_address {
				config.service.hive_address = hive_address;
			}
		}

		if let Some(storage) = self.storage {
			if let Some(repository_path) = storage.repository_path {
				config.storage.repository_path = repository_path;
			}
			if let Some(upload_cache_path) = storage.upload_cache_path {
				config.storage.upload_cache_path = upload_cache_path;
			}
		}

		if let Some(security) = self.security {
			if let Some(jwt_secret) = security.jwt_secret {
				config.security.jwt_secret = jwt_secret;
			}
		}

		config
	}
}

pub fn init_from_args() -> Result<LoadedConfig, ConfigError> {
	let config_path = parse_config_path_from_args(std::env::args_os())?;
	let loaded = match config_path {
		Some(path) => load_from_file(path)?,
		None => load_default_or_defaults(DEFAULT_CONFIG_PATH)?,
	};

	CONFIG
		.set(loaded.config.clone())
		.map_err(|_| ConfigError::AlreadyInitialized)?;

	Ok(loaded)
}

pub fn current() -> &'static AppConfig {
	CONFIG
		.get()
		.expect("crv-hive config accessed before initialization")
}

pub fn load_from_file(path: impl Into<PathBuf>) -> Result<LoadedConfig, ConfigError> {
	let path = path.into();
	let contents = fs::read_to_string(&path).map_err(|source| ConfigError::ReadConfig {
		path: path.clone(),
		source,
	})?;
	let raw = toml::from_str::<RawAppConfig>(&contents).map_err(|source| {
		ConfigError::ParseConfig {
			path: path.clone(),
			source,
		}
	})?;

	Ok(LoadedConfig {
		config: raw.into_config(),
		source: ConfigSource::File(path),
	})
}

fn load_default_or_defaults(path: impl AsRef<Path>) -> Result<LoadedConfig, ConfigError> {
	let path = path.as_ref();
	if path.is_file() {
		return load_from_file(path.to_path_buf());
	}

	Ok(LoadedConfig {
		config: AppConfig::default(),
		source: ConfigSource::Defaults,
	})
}

fn parse_config_path_from_args<I, T>(args: I) -> Result<Option<PathBuf>, ConfigError>
where
	I: IntoIterator<Item = T>,
	T: Into<OsString>,
{
	let mut args = args.into_iter().map(Into::into);
	let _ = args.next();

	let mut config_path = None;
	while let Some(arg) = args.next() {
		let arg_text = arg.to_string_lossy();
		match arg_text.as_ref() {
			"-c" | "--config" => {
				let Some(path) = args.next() else {
					return Err(ConfigError::MissingConfigPath);
				};
				config_path = Some(PathBuf::from(path));
			}
			_ if arg_text.starts_with("-c=") => {
				config_path = Some(PathBuf::from(&arg_text[3..]));
			}
			_ if arg_text.starts_with("--config=") => {
				config_path = Some(PathBuf::from(&arg_text[9..]));
			}
			"-h" | "--help" => {}
			_ if arg_text.starts_with('-') => {
				return Err(ConfigError::UnsupportedArgument(arg_text.into_owned()));
			}
			_ => {
				return Err(ConfigError::UnsupportedArgument(arg_text.into_owned()));
			}
		}
	}

	Ok(config_path)
}

#[cfg(test)]
mod tests {
	use super::{AppConfig, ConfigSource, RawAppConfig, load_from_file, parse_config_path_from_args};
	use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

	#[test]
	fn partial_nested_config_falls_back_to_defaults() {
		let raw = toml::from_str::<RawAppConfig>(
			r#"
				[logging]
				rust_log = "debug"

				[iroh]
				relay_url = "http://127.0.0.1:4455"
			"#,
		)
		.expect("config should deserialize");

		let config = raw.into_config();

		assert_eq!(config.logging.rust_log, "debug");
		assert_eq!(config.iroh.relay_url, "http://127.0.0.1:4455");
		assert_eq!(config.iroh.relay_bind_addr, AppConfig::default().iroh.relay_bind_addr);
		assert_eq!(
			config.database.max_connections,
			AppConfig::default().database.max_connections
		);
	}

	#[test]
	fn cli_supports_config_flag() {
		let config_path = parse_config_path_from_args([
			OsString::from("crv-hive"),
			OsString::from("-c"),
			OsString::from("custom.toml"),
		])
		.expect("cli args should parse");

		assert_eq!(config_path, Some(PathBuf::from("custom.toml")));
	}

	#[test]
	fn file_loading_marks_source() {
		let path = temp_file("crv-hive-config", "[logging]\nrust_log = \"trace\"\n");

		let loaded = load_from_file(&path).expect("config file should load");

		match loaded.source {
			ConfigSource::File(actual_path) => assert_eq!(actual_path, path),
			ConfigSource::Defaults => panic!("expected file source"),
		}
		assert_eq!(loaded.config.logging.rust_log, "trace");

		let _ = fs::remove_file(path);
	}

	fn temp_file(prefix: &str, contents: &str) -> PathBuf {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.expect("system time should be valid")
			.as_nanos();
		let path = std::env::temp_dir().join(format!("{prefix}-{nanos}.toml"));
		fs::write(&path, contents).expect("temp config file should be writable");
		path
	}

	use std::ffi::OsString;
}
