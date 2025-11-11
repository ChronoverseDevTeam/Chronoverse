mod adapter;
mod client;
mod error;
mod minio_adapter;

pub use adapter::S3Adapter;
pub use client::{get_s3_client, init_s3_client};
pub use error::S3Error;
pub use minio_adapter::MinioAdapter;

/// S3 操作结果类型别名
pub type S3Result<T> = Result<T, S3Error>;

