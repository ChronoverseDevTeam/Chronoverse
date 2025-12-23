use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigEntity {
    pub postgres_hostname: String,
    pub postgres_database: String,
    pub postgres_username: String,
    pub postgres_password: String,
    pub postgres_port: u16, 

    pub hive_address: Option<String>,
    pub repository_path: String,
    pub jwt_secret: String,
}

impl Default for ConfigEntity {
    fn default() -> Self {
        Self {
            // 添加 directConnection=true&w=1 用于单节点 MongoDB（非副本集）
            postgres_hostname: "127.0.0.1".to_string(),
            postgres_database: "chronoverse".to_string(),
            postgres_username: "postgres".to_string(),
            postgres_password: "postgres".to_string(),
            postgres_port: 5432,

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
