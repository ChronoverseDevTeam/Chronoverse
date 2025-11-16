use std::fmt;

/// S3 客户端错误类型
#[derive(Debug)]
pub enum S3Error {
    /// 配置错误
    ConfigError(String),
    /// 连接错误
    ConnectionError(String),
    /// 认证错误
    AuthenticationError(String),
    /// 存储桶不存在或无法访问
    BucketError(String),
    /// 对象操作错误
    ObjectError(String),
    /// 未初始化
    NotInitialized,
    /// 其他错误
    Other(String),
}

impl fmt::Display for S3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            S3Error::ConfigError(msg) => write!(f, "S3 配置错误: {}", msg),
            S3Error::ConnectionError(msg) => write!(f, "S3 连接错误: {}", msg),
            S3Error::AuthenticationError(msg) => write!(f, "S3 认证错误: {}", msg),
            S3Error::BucketError(msg) => write!(f, "S3 存储桶错误: {}", msg),
            S3Error::ObjectError(msg) => write!(f, "S3 对象操作错误: {}", msg),
            S3Error::NotInitialized => write!(f, "S3 客户端未初始化"),
            S3Error::Other(msg) => write!(f, "S3 其他错误: {}", msg),
        }
    }
}

impl std::error::Error for S3Error {}

impl From<minio::s3::error::Error> for S3Error {
    fn from(err: minio::s3::error::Error) -> Self {
        S3Error::Other(err.to_string())
    }
}
