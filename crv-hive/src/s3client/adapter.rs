use async_trait::async_trait;
use bytes::Bytes;

use super::S3Result;

/// S3 适配器 trait，定义了 S3 客户端的抽象接口
/// 
/// 任何实现此 trait 的类型都可以作为 S3 客户端使用，
/// 这样可以轻松替换不同的 S3 提供商（MinIO、AWS S3、阿里云 OSS 等）
#[async_trait]
pub trait S3Adapter: Send + Sync {
    /// 上传对象到 S3
    /// 
    /// # 参数
    /// * `bucket` - 存储桶名称
    /// * `object_name` - 对象名称（文件路径）
    /// * `data` - 要上传的数据
    /// * `content_type` - 内容类型（如 "image/png"）
    /// 
    /// # 返回
    /// 成功返回 ()，失败返回 S3Error
    async fn put_object(
        &self,
        bucket: &str,
        object_name: &str,
        data: Bytes,
        content_type: Option<&str>,
    ) -> S3Result<()>;

    /// 从 S3 获取对象
    /// 
    /// # 参数
    /// * `bucket` - 存储桶名称
    /// * `object_name` - 对象名称（文件路径）
    /// 
    /// # 返回
    /// 成功返回对象数据，失败返回 S3Error
    async fn get_object(&self, bucket: &str, object_name: &str) -> S3Result<Bytes>;

    /// 删除 S3 对象
    /// 
    /// # 参数
    /// * `bucket` - 存储桶名称
    /// * `object_name` - 对象名称（文件路径）
    /// 
    /// # 返回
    /// 成功返回 ()，失败返回 S3Error
    async fn delete_object(&self, bucket: &str, object_name: &str) -> S3Result<()>;

    /// 检查对象是否存在
    /// 
    /// # 参数
    /// * `bucket` - 存储桶名称
    /// * `object_name` - 对象名称（文件路径）
    /// 
    /// # 返回
    /// 成功返回布尔值，失败返回 S3Error
    async fn object_exists(&self, bucket: &str, object_name: &str) -> S3Result<bool>;

    /// 列出指定前缀的对象
    /// 
    /// # 参数
    /// * `bucket` - 存储桶名称
    /// * `prefix` - 对象前缀
    /// 
    /// # 返回
    /// 成功返回对象名称列表，失败返回 S3Error
    async fn list_objects(&self, bucket: &str, prefix: &str) -> S3Result<Vec<String>>;

    /// 确保存储桶存在，如果不存在则创建
    /// 
    /// # 参数
    /// * `bucket` - 存储桶名称
    /// 
    /// # 返回
    /// 成功返回 ()，失败返回 S3Error
    async fn ensure_bucket(&self, bucket: &str) -> S3Result<()>;
}

