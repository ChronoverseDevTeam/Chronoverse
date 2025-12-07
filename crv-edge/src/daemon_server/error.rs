//! 统一的错误处理
use thiserror::Error;
use tonic::Status;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Db(#[from] super::db::DbError),

    #[error("Invalid configuration: {0}")]
    Config(String),

    #[error("Raw error: {0}")]
    Raw(Status),

    #[error("Internal server error")]
    Unknown,
}

impl From<Status> for AppError {
    fn from(value: Status) -> Self {
        Self::Raw(value)
    }
}

// 自动将业务错误转换为 gRPC Status
impl From<AppError> for Status {
    fn from(err: AppError) -> Self {
        match err {
            AppError::Db(e) => Status::internal(format!("DB Error: {}", e)),
            AppError::Config(msg) => Status::invalid_argument(msg),
            AppError::Unknown => Status::internal("Unknown error"),
            AppError::Raw(status) => status,
        }
    }
}
