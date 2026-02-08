use super::{LogConfig, LogFormat, LogLevel, LogOutput, LogRotation, Logger};
use std::path::PathBuf;

pub fn init_default_logger() -> Result<super::LoggerGuard, super::LogError> {
    Logger::builder()
        .level(LogLevel::Info)
        .format(LogFormat::Compact)
        .stdout()
        .build()
        .init()
}

pub fn init_file_logger(log_dir: PathBuf) -> Result<super::LoggerGuard, super::LogError> {
    Logger::builder()
        .level(LogLevel::Debug)
        .format(LogFormat::Json)
        .file(log_dir, LogRotation::Daily)
        .build()
        .init()
}

pub fn init_both_logger(log_dir: PathBuf) -> Result<super::LoggerGuard, super::LogError> {
    Logger::builder()
        .level(LogLevel::Info)
        .format(LogFormat::Pretty)
        .both(log_dir, LogRotation::Daily)
        .build()
        .init()
}

pub fn init_custom_logger() -> Result<super::LoggerGuard, super::LogError> {
    let config = LogConfig {
        level: LogLevel::Trace,
        format: LogFormat::Json,
        output: LogOutput::Stdout,
    };
    
    Logger::new(config).init()
}

