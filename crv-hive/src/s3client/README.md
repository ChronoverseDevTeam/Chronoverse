# S3 客户端抽象层

这个模块提供了一个基于适配器模式的 S3 抽象调用层，允许轻松替换不同的 S3 提供商。

## 设计特点

- **适配器模式**: 通过 `S3Adapter` trait 定义统一的接口，不同的 S3 提供商实现此接口
- **全局单例**: 使用 `OnceLock` 维护全局客户端实例，避免重复创建
- **可替换性**: 当前实现使用 MinIO，未来可以轻松替换为 AWS S3、阿里云 OSS 等
- **类型安全**: 使用 Rust 的类型系统确保编译时的安全性

## 架构

```
┌─────────────────────┐
│   Other Modules     │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  get_s3_client()    │  ◄─── 全局访问点
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│   S3Adapter trait   │  ◄─── 抽象接口
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│   MinioAdapter      │  ◄─── 当前实现
└─────────────────────┘
```

## 使用方法

### 1. 初始化客户端

在应用启动时调用 `init_s3_client()`：

```rust
use crv_hive::s3client::init_s3_client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化 S3 客户端（从配置中读取参数）
    init_s3_client().await?;
    
    // ... 其他初始化代码
    
    Ok(())
}
```

### 2. 使用客户端

在任何模块中使用 `get_s3_client()` 获取客户端引用：

```rust
use crv_hive::s3client::get_s3_client;
use bytes::Bytes;

async fn upload_file() -> Result<(), Box<dyn std::error::Error>> {
    // 获取全局 S3 客户端
    let s3 = get_s3_client()?;
    
    // 上传文件
    let data = Bytes::from("Hello, S3!");
    s3.put_object("my-bucket", "test.txt", data, Some("text/plain")).await?;
    
    Ok(())
}

async fn download_file() -> Result<(), Box<dyn std::error::Error>> {
    let s3 = get_s3_client()?;
    
    // 下载文件
    let data = s3.get_object("my-bucket", "test.txt").await?;
    println!("Downloaded {} bytes", data.len());
    
    Ok(())
}

async fn check_file_exists() -> Result<(), Box<dyn std::error::Error>> {
    let s3 = get_s3_client()?;
    
    // 检查文件是否存在
    let exists = s3.object_exists("my-bucket", "test.txt").await?;
    println!("File exists: {}", exists);
    
    Ok(())
}

async fn delete_file() -> Result<(), Box<dyn std::error::Error>> {
    let s3 = get_s3_client()?;
    
    // 删除文件
    s3.delete_object("my-bucket", "test.txt").await?;
    
    Ok(())
}
```

## API 参考

### `S3Adapter` trait

所有 S3 适配器必须实现的接口：

- `put_object`: 上传对象到 S3
- `get_object`: 从 S3 获取对象
- `delete_object`: 删除 S3 对象
- `object_exists`: 检查对象是否存在
- `list_objects`: 列出指定前缀的对象
- `ensure_bucket`: 确保存储桶存在，如果不存在则创建

### `MinioAdapter`

MinIO S3 适配器的具体实现。

## 配置

S3 客户端从 `ConfigEntity` 中读取以下配置项：

- `s3_endpoint`: S3 服务端点（如 "http://localhost:9000"）
- `s3_region`: S3 区域（如 "us-east-1"）
- `s3_access_key`: 访问密钥
- `s3_secret_key`: 密钥
- `s3_bucket`: 默认存储桶名称

## 扩展：添加新的 S3 提供商

要添加新的 S3 提供商（如 AWS S3），只需：

1. 创建新的适配器结构体（如 `AwsS3Adapter`）
2. 实现 `S3Adapter` trait
3. 在 `init_s3_client()` 中选择使用哪个适配器

示例：

```rust
// 在 s3client/aws_adapter.rs 中
pub struct AwsS3Adapter {
    client: aws_sdk_s3::Client,
    // ...
}

#[async_trait]
impl S3Adapter for AwsS3Adapter {
    // 实现所有 trait 方法
    // ...
}

// 在 s3client/client.rs 中
pub async fn init_s3_client() -> S3Result<()> {
    let config = get_or_init_config();
    
    // 根据配置选择适配器
    let adapter: Box<dyn S3Adapter> = if use_aws {
        Box::new(AwsS3Adapter::from_config(config)?)
    } else {
        Box::new(MinioAdapter::from_config(config)?)
    };
    
    // ...
}
```

## 注意事项

- 必须在使用任何 S3 功能前调用 `init_s3_client()`
- `list_objects` 方法目前返回空列表，需要进一步研究 minio 0.3.0 的 API 来完善
- 客户端是线程安全的，可以在多个线程中共享使用

