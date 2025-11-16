/// S3 客户端使用示例
///
/// 此示例展示如何使用 S3 客户端进行常见的对象存储操作
///
/// 运行此示例：
/// ```bash
/// cargo run --example s3_usage
/// ```
use bytes::Bytes;
use crv_hive::{config, s3client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== S3 客户端使用示例 ===\n");

    // 1. 加载配置
    println!("1. 加载配置...");
    config::holder::load_config().await?;
    println!("   ✓ 配置加载成功\n");

    // 2. 初始化 S3 客户端
    println!("2. 初始化 S3 客户端...");
    s3client::init_s3_client().await?;
    println!("   ✓ S3 客户端初始化成功\n");

    // 3. 获取 S3 客户端
    println!("3. 获取 S3 客户端引用...");
    let s3 = s3client::get_s3_client()?;
    println!("   ✓ 获取客户端成功\n");

    // 获取配置中的默认存储桶
    let config = config::holder::get_config().unwrap();
    let bucket = &config.s3_bucket;
    println!("   使用存储桶: {}\n", bucket);

    // 4. 上传对象
    println!("4. 上传对象...");
    let test_data = Bytes::from("Hello, S3! 这是一个测试文件。");
    let object_name = "test/example.txt";

    s3.put_object(bucket, object_name, test_data.clone(), Some("text/plain"))
        .await?;
    println!("   ✓ 上传成功: {}", object_name);
    println!("   大小: {} bytes\n", test_data.len());

    // 5. 检查对象是否存在
    println!("5. 检查对象是否存在...");
    let exists = s3.object_exists(bucket, object_name).await?;
    println!("   ✓ 对象存在: {}\n", exists);

    // 6. 下载对象
    println!("6. 下载对象...");
    let downloaded_data = s3.get_object(bucket, object_name).await?;
    println!("   ✓ 下载成功");
    println!("   大小: {} bytes", downloaded_data.len());
    println!("   内容: {}\n", String::from_utf8_lossy(&downloaded_data));

    // 7. 列出对象（注意：当前实现返回空列表）
    println!("7. 列出对象...");
    let objects = s3.list_objects(bucket, "test/").await?;
    println!("   ✓ 找到 {} 个对象\n", objects.len());
    for obj in objects {
        println!("      - {}", obj);
    }

    // 8. 删除对象
    println!("8. 删除对象...");
    s3.delete_object(bucket, object_name).await?;
    println!("   ✓ 删除成功: {}\n", object_name);

    // 9. 再次检查对象是否存在
    println!("9. 验证删除...");
    let exists_after_delete = s3.object_exists(bucket, object_name).await?;
    println!("   ✓ 对象存在: {}\n", exists_after_delete);

    println!("=== 示例完成 ===");

    Ok(())
}
