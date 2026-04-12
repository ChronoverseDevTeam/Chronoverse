use std::{
	ffi::OsString,
	path::PathBuf,
};

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_CONFIG_PATH: &str = "relay.toml";

static CONFIG: OnceCell<RelayAppConfig> = OnceCell::new();

#[derive(Debug, Clone)]
pub enum ConfigSource {
	Defaults,
	File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
	pub config: RelayAppConfig,
	pub source: ConfigSource,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RelayAppConfig {
	pub logging: LoggingConfig,
	pub relay: RelayConfig,
}

impl Default for RelayAppConfig {
	fn default() -> Self {
		Self {
			logging: LoggingConfig::default(),
			relay: RelayConfig::default(),
		}
	}
}

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct RelayConfig {
	pub relay_bind_addr: String,
	pub captive_portal_addr: String,
}

impl Default for RelayConfig {
	fn default() -> Self {
		Self {
			relay_bind_addr: "0.0.0.0:3340".to_owned(),
			captive_portal_addr: "0.0.0.0:80".to_owned(),
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
	#[error("failed to write default config file {path}: {source}")]
	WriteConfig {
		path: PathBuf,
		#[source]
		source: std::io::Error,
	},
	#[error("failed to serialize default config: {source}")]
	SerializeConfig {
		#[source]
		source: toml::ser::Error,
	},
	#[error("application config has already been initialized")]
	AlreadyInitialized,
}

#[derive(Debug, Default, Deserialize)]
struct RawAppConfig {
	#[serde(default)]
	logging: Option<RawLoggingConfig>,
	#[serde(default)]
	relay: Option<RawRelayConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct RawLoggingConfig {
	rust_log: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawRelayConfig {
	relay_bind_addr: Option<String>,
	captive_portal_addr: Option<String>,
}

impl RawAppConfig {
	fn into_config(self) -> RelayAppConfig {
		let mut config = RelayAppConfig::default();

		if let Some(logging) = self.logging {
			if let Some(rust_log) = logging.rust_log {
				config.logging.rust_log = rust_log;
			}
		}

		if let Some(relay) = self.relay {
			if let Some(relay_bind_addr) = relay.relay_bind_addr {
				config.relay.relay_bind_addr = relay_bind_addr;
			}
			if let Some(captive_portal_addr) = relay.captive_portal_addr {
				config.relay.captive_portal_addr = captive_portal_addr;
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

pub fn current() -> &'static RelayAppConfig {
	CONFIG
		.get()
		.expect("crv-relay config accessed before initialization")
}

pub fn load_from_file(path: impl Into<PathBuf>) -> Result<LoadedConfig, ConfigError> {
	let path = path.into();
	let contents = std::fs::read_to_string(&path).map_err(|source| ConfigError::ReadConfig {
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

fn load_default_or_defaults(filename: &str) -> Result<LoadedConfig, ConfigError> {
	// Prefer a config file that lives next to the executable so the binary is
	// self-contained and works regardless of the working directory.
	let path = std::env::current_exe()
		.ok()
		.and_then(|exe| exe.parent().map(|p| p.join(filename)))
		.unwrap_or_else(|| PathBuf::from(filename));

	if !path.is_file() {
		let content = toml::to_string_pretty(&RelayAppConfig::default())
			.map_err(|source| ConfigError::SerializeConfig { source })?;
		std::fs::write(&path, content).map_err(|source| {
			ConfigError::WriteConfig { path: path.clone(), source }
		})?;
		tracing::info!("created default config at {}", path.display());
	}

	load_from_file(path)
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
			other if other.starts_with('-') => {
				return Err(ConfigError::UnsupportedArgument(other.to_owned()));
			}
			_ if arg_text.ends_with(".toml") => {
				config_path = Some(PathBuf::from(&*arg_text));
			}
			_ => {}
		}
	}

	Ok(config_path)
}
