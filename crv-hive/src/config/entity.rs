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
    pub repository_path: String,
    pub jwt_secret: String,
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
            repository_path: default_repository_path(),
            jwt_secret: "dev-secret".to_string(),
        }
    }
}

fn default_repository_path() -> String {
    if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let mut path = std::path::PathBuf::from(appdata);
            path.push("crv");
            path.push("shards");
            path.to_string_lossy().into_owned()
        } else {
            "%AppData%/crv/shards".to_string()
        }
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "~".to_string());
        format!("{home}/.crv/shards")
    }
}
