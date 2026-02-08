mod config;
mod writer;
mod macros;
pub mod example;
pub mod rotation;
pub mod reader;
pub mod recovery;
pub mod test;

pub use config::{LogConfig, LogFormat, LogLevel, LogOutput};
pub use writer::LogWriter;
pub use rotation::LineBasedRotation;
pub use reader::LogReader;
pub use recovery::{RecoveryLog, RecoveryState, LogEntry};

pub use tracing;
pub use tracing::{trace, debug, info, warn, error, instrument, span, event};

use std::path::PathBuf;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

pub struct Logger {
    config: LogConfig,
}

impl Logger {
    pub fn new(config: LogConfig) -> Self {
        Self { config }
    }

    pub fn builder() -> LoggerBuilder {
        LoggerBuilder::default()
    }

    pub fn init(self) -> Result<LoggerGuard, LogError> {
        let filter = self.build_filter();
        
        match &self.config.output {
            LogOutput::Stdout => {
                self.init_stdout(filter)?;
                Ok(LoggerGuard { _guards: vec![] })
            }
            LogOutput::File { path, rotation } => {
                let guard = self.init_file(filter, path, rotation)?;
                Ok(LoggerGuard { _guards: vec![guard] })
            }
            LogOutput::Both { path, rotation } => {
                let guard = self.init_both(filter, path, rotation)?;
                Ok(LoggerGuard { _guards: vec![guard] })
            }
        }
    }

    fn build_filter(&self) -> EnvFilter {
        let level_str = match self.config.level {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        };

        EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new(level_str))
            .unwrap_or_else(|_| EnvFilter::new("info"))
    }

    fn init_stdout(&self, filter: EnvFilter) -> Result<(), LogError> {
        match self.config.format {
            LogFormat::Json => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .json()
                            .with_span_events(FmtSpan::CLOSE)
                            .with_current_span(true)
                            .with_target(true)
                            .with_thread_ids(true)
                            .with_thread_names(true),
                    )
                    .init();
            }
            LogFormat::Pretty => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .pretty()
                            .with_span_events(FmtSpan::CLOSE)
                            .with_target(true),
                    )
                    .init();
            }
            LogFormat::Compact => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .compact()
                            .with_target(true),
                    )
                    .init();
            }
        }
        Ok(())
    }

    fn init_file(
        &self,
        filter: EnvFilter,
        path: &PathBuf,
        rotation: &LogRotation,
    ) -> Result<tracing_appender::non_blocking::WorkerGuard, LogError> {
        let file_appender = self.create_file_appender(path, rotation)?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        match self.config.format {
            LogFormat::Json => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .json()
                            .with_writer(non_blocking)
                            .with_span_events(FmtSpan::CLOSE)
                            .with_current_span(true)
                            .with_target(true)
                            .with_thread_ids(true)
                            .with_thread_names(true)
                            .with_ansi(false),
                    )
                    .init();
            }
            LogFormat::Pretty => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .with_writer(non_blocking)
                            .with_span_events(FmtSpan::CLOSE)
                            .with_target(true)
                            .with_ansi(false),
                    )
                    .init();
            }
            LogFormat::Compact => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(
                        fmt::layer()
                            .compact()
                            .with_writer(non_blocking)
                            .with_target(true)
                            .with_ansi(false),
                    )
                    .init();
            }
        }

        Ok(guard)
    }

    fn init_both(
        &self,
        filter: EnvFilter,
        path: &PathBuf,
        rotation: &LogRotation,
    ) -> Result<tracing_appender::non_blocking::WorkerGuard, LogError> {
        let file_appender = self.create_file_appender(path, rotation)?;
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        match self.config.format {
            LogFormat::Json => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().json())
                    .with(
                        fmt::layer()
                            .json()
                            .with_writer(non_blocking)
                            .with_ansi(false),
                    )
                    .init();
            }
            LogFormat::Pretty => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().pretty())
                    .with(
                        fmt::layer()
                            .with_writer(non_blocking)
                            .with_ansi(false),
                    )
                    .init();
            }
            LogFormat::Compact => {
                tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt::layer().compact())
                    .with(
                        fmt::layer()
                            .compact()
                            .with_writer(non_blocking)
                            .with_ansi(false),
                    )
                    .init();
            }
        }

        Ok(guard)
    }

    fn create_file_appender(
        &self,
        path: &PathBuf,
        rotation: &LogRotation,
    ) -> Result<tracing_appender::rolling::RollingFileAppender, LogError> {
        let dir = path.parent().ok_or_else(|| {
            LogError::InvalidPath(format!("Invalid log path: {}", path.display()))
        })?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("chronoverse.log");

        let appender = match rotation {
            LogRotation::Hourly => tracing_appender::rolling::hourly(dir, file_name),
            LogRotation::Daily => tracing_appender::rolling::daily(dir, file_name),
            LogRotation::Never => tracing_appender::rolling::never(dir, file_name),
            LogRotation::SizeBased { max_size_mb } => {
                return Err(LogError::UnsupportedRotation(format!(
                    "Size-based rotation ({}MB) not yet implemented",
                    max_size_mb
                )));
            }
        };

        Ok(appender)
    }
}

pub struct LoggerGuard {
    _guards: Vec<tracing_appender::non_blocking::WorkerGuard>,
}

#[derive(Default)]
pub struct LoggerBuilder {
    level: Option<LogLevel>,
    format: Option<LogFormat>,
    output: Option<LogOutput>,
}

impl LoggerBuilder {
    pub fn level(mut self, level: LogLevel) -> Self {
        self.level = Some(level);
        self
    }

    pub fn format(mut self, format: LogFormat) -> Self {
        self.format = Some(format);
        self
    }

    pub fn output(mut self, output: LogOutput) -> Self {
        self.output = Some(output);
        self
    }

    pub fn stdout(self) -> Self {
        self.output(LogOutput::Stdout)
    }

    pub fn file(self, path: PathBuf, rotation: LogRotation) -> Self {
        self.output(LogOutput::File { path, rotation })
    }

    pub fn both(self, path: PathBuf, rotation: LogRotation) -> Self {
        self.output(LogOutput::Both { path, rotation })
    }

    pub fn build(self) -> Logger {
        Logger::new(LogConfig {
            level: self.level.unwrap_or(LogLevel::Info),
            format: self.format.unwrap_or(LogFormat::Compact),
            output: self.output.unwrap_or(LogOutput::Stdout),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogRotation {
    Hourly,
    Daily,
    Never,
    SizeBased { max_size_mb: u64 },
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid log path: {0}")]
    InvalidPath(String),

    #[error("Unsupported rotation strategy: {0}")]
    UnsupportedRotation(String),

    #[error("Logger already initialized")]
    AlreadyInitialized,
}
