pub mod changelist;
pub mod file;
pub mod file_revision;
pub mod submit;
pub mod user;

use sea_orm::DbErr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DaoError {
    #[error("database error: {0}")]
    Db(#[from] DbErr),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type DaoResult<T> = Result<T, DaoError>;
