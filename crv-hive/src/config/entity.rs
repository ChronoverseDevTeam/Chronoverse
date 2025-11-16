use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigEntity {
    pub mongo_url: String,
    pub mongo_database: String,
    pub mongo_app: Option<String>,
    pub mongo_username: Option<String>,
    pub mongo_password: Option<String>,

    pub hive_address: Option<String>,
    pub jwt_secret: String,

    pub s3_endpoint: String,
    pub s3_region: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub s3_bucket: String,
}

impl Default for ConfigEntity {
    fn default() -> Self {
        Self {
            // 添加 directConnection=true&w=1 用于单节点 MongoDB（非副本集）
            mongo_url: "mongodb://127.0.0.1:27017/?directConnection=true&w=1".to_string(),
            mongo_database: "chronoverse".to_string(),
            mongo_app: Some("Chronoverse".to_string()),
            mongo_username: None,
            mongo_password: None,

            hive_address: Some("0.0.0.0:34560".to_string()),
            jwt_secret: "dev-secret".to_string(),

            s3_endpoint: "http://localhost:9000".to_string(),
            s3_region: "us-east-1".to_string(),
            s3_access_key: "minioadmin".to_string(),
            s3_secret_key: "minioadmin".to_string(),
            s3_bucket: "crv-file-blocks".to_string(),
        }
    }
}
