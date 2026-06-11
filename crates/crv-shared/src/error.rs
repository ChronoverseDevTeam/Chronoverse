use thiserror::Error;

#[derive(Error, Debug)]
pub enum CrvError {
    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("not authorized: {0}")]
    NotAuthorized(String),

    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("resource already exists: {0}")]
    AlreadyExists(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("file is locked by '{user}' in workspace '{client}'")]
    FileLocked { user: String, client: String },

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, CrvError>;

// ── From impls for ergonomic `?` usage ──────────────────────────

impl From<std::io::Error> for CrvError {
    fn from(e: std::io::Error) -> Self {
        CrvError::Storage(e.to_string())
    }
}

impl From<serde_json::Error> for CrvError {
    fn from(e: serde_json::Error) -> Self {
        CrvError::Internal(format!("serialization error: {e}"))
    }
}

impl From<uuid::Error> for CrvError {
    fn from(e: uuid::Error) -> Self {
        CrvError::InvalidInput(format!("invalid UUID: {e}"))
    }
}
