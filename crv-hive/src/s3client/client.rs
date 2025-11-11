use std::sync::OnceLock;

use crate::config::holder::get_or_init_config;

use super::adapter::S3Adapter;
use super::error::S3Error;
use super::minio_adapter::MinioAdapter;
use super::S3Result;

/// 全局 S3 客户端实例
/// 
/// 使用 OnceLock 确保线程安全的单例模式
static S3_CLIENT: OnceLock<Box<dyn S3Adapter>> = OnceLock::new();

/// 初始化 S3 客户端
/// 
/// 从配置中读取 S3 相关配置，创建 MinioAdapter 实例，
/// 并将其设置为全局客户端。
/// 
/// # 返回
/// 成功返回 ()，失败返回 S3Error
/// 
/// # 示例
/// ```no_run
/// use crv_hive::s3client::init_s3_client;
/// 
/// #[tokio::main]
/// async fn main() {
///     init_s3_client().await.expect("初始化 S3 客户端失败");
/// }
/// ```
pub async fn init_s3_client() -> S3Result<()> {
    let config = get_or_init_config();

    // 创建 MinIO 适配器
    let adapter = MinioAdapter::from_config(config)?;

    // 确保默认存储桶存在
    adapter.ensure_bucket(adapter.default_bucket()).await?;

    // 将适配器装箱并设置为全局实例
    let boxed_adapter: Box<dyn S3Adapter> = Box::new(adapter);
    
    S3_CLIENT
        .set(boxed_adapter)
        .map_err(|_| S3Error::Other("S3 客户端已经初始化".to_string()))?;

    Ok(())
}

/// 获取全局 S3 客户端引用
/// 
/// 如果客户端未初始化，将返回 S3Error::NotInitialized 错误。
/// 
/// # 返回
/// 成功返回 S3 客户端的引用，失败返回 S3Error
/// 
/// # 示例
/// ```no_run
/// use crv_hive::s3client::get_s3_client;
/// use bytes::Bytes;
/// 
/// #[tokio::main]
/// async fn main() {
///     let client = get_s3_client().expect("获取 S3 客户端失败");
///     
///     // 上传文件
///     let data = Bytes::from("Hello, S3!");
///     client.put_object("my-bucket", "test.txt", data, Some("text/plain"))
///         .await
///         .expect("上传文件失败");
/// }
/// ```
pub fn get_s3_client() -> S3Result<&'static dyn S3Adapter> {
    S3_CLIENT
        .get()
        .map(|client| client.as_ref())
        .ok_or(S3Error::NotInitialized)
}

