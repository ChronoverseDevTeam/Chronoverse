use std::sync::OnceLock;

use mongodb::options::ClientOptions;
use mongodb::{Client, Collection, Database};
use crate::config::holder::get_or_init_config;
use crate::config::entity::ConfigEntity;

fn inject_auth_in_uri(base_uri: &str, username: &str, password: Option<&str>) -> String {
    if base_uri.contains('@') { return base_uri.to_string(); }
    if let Some(idx) = base_uri.find("://") {
        let (scheme, rest) = base_uri.split_at(idx);
        let rest = &rest[3..]; // skip ://
        let auth = match password {
            Some(pw) => format!("{}:{}@", username, pw),
            None => format!("{}@", username),
        };
        format!("{}://{}{}", scheme, auth, rest)
    } else {
        base_uri.to_string()
    }
}

#[derive(Debug)]
pub enum MongoError {
    AlreadyInitialized,
    Mongo(mongodb::error::Error),
}

impl std::fmt::Display for MongoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MongoError::AlreadyInitialized => write!(f, "MongoDB 已初始化"),
            MongoError::Mongo(e) => write!(f, "Mongo 错误: {}", e),
        }
    }
}

impl std::error::Error for MongoError {}

impl From<mongodb::error::Error> for MongoError {
    fn from(value: mongodb::error::Error) -> Self { Self::Mongo(value) }
}

pub type Result<T> = std::result::Result<T, MongoError>;

#[derive(Clone)]
pub struct MongoManager {
    client: Client,
    database: Database,
}

impl MongoManager {
    pub async fn connect_from_entity(cfg: &ConfigEntity) -> mongodb::error::Result<Self> {
        let mut uri = cfg.mongo_url.clone();
        if let Some(user) = &cfg.mongo_username {
            let pw = cfg.mongo_password.as_deref();
            uri = inject_auth_in_uri(&uri, user, pw);
        }

        let mut options = ClientOptions::parse(&uri).await?;
        if let Some(app) = &cfg.mongo_app {
            options.app_name = Some(app.clone());
        }

        let client = Client::with_options(options)?;
        let database = client.database(&cfg.mongo_database);
        Ok(Self { client, database })
    }

    pub fn database(&self) -> Database { self.database.clone() }

    pub fn collection<T>(&self, name: &str) -> Collection<T>
    where
        T: Send + Sync + Unpin + serde::de::DeserializeOwned + serde::Serialize,
    {
        self.database.collection::<T>(name)
    }

    pub fn client(&self) -> Client { self.client.clone() }
}

static MONGO: OnceLock<MongoManager> = OnceLock::new();

pub async fn init_mongo_with_config(cfg: &ConfigEntity) -> Result<()> {
    if MONGO.get().is_some() {
        return Err(MongoError::AlreadyInitialized);
    }

    let manager = MongoManager::connect_from_entity(cfg).await?;
    MONGO.set(manager).map_err(|_| MongoError::AlreadyInitialized)?;
    Ok(())
}

pub async fn init_mongo_from_config() -> Result<()> {
    let cfg = get_or_init_config();
    init_mongo_with_config(cfg).await
}


pub fn get_mongo() -> &'static MongoManager {
    MONGO.get().expect("MongoDB 未初始化，请先调用 init_mongo_from_config 或 init_mongo_with_config")
}

/// 关闭全局 Mongo 连接（在优雅关停时调用）。
/// 注意：mongodb Rust 驱动的 Client/Database 是轻量的句柄，关闭通常是释放资源即可。
pub async fn shutdown_mongo() {
    // 将 OnceLock 中的管理器泄露引用转为拥有权并丢弃（如果需要的话）。
    // OnceLock 不支持 take，这里通过不再使用句柄让其在进程结束时释放。
    // 如后续需要更严格的关闭，可在此添加驱动提供的显式关闭（若提供）。
}

