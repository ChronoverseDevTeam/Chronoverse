use thiserror::Error;
use crate::cas::BlobId;

/// All errors that can be returned by the CAS store.
#[derive(Debug, Error)]
pub enum CasError {
    /// The requested blob was not found in the store.
    #[error("blob not found: {0}")]
    NotFound(BlobId),

    /// A filesystem / network I/O error originating from our own operations.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// The underlying iroh-blobs store or RPC transport returned an error.
    #[error("store error: {0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl CasError {
    /// Wrap any store/transport error.
    pub(crate) fn store(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        CasError::Store(Box::new(e))
    }
}

pub type Result<T> = std::result::Result<T, CasError>;
