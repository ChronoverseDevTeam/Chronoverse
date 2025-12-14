pub mod dao;

use std::sync::{Arc, Mutex, OnceLock};

use mongodb::{
    bson::doc,
    options::{ClientOptions, Credential},
    Client, Database,
};

use crate::config::holder::get_or_init_config;

/// MongoDB 访问单例的内部结构
#[derive(Clone)]
pub struct MongoManager {
    client: Client,
    db: Database,
}

impl MongoManager {
    /// 获取底层 `Client`
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// 获取默认的 `Database`
    ///
    /// `mongodb::Database` 是一个轻量的句柄，实现了 `Clone`，
    /// 这里直接返回克隆副本，便于在各处复用。
    pub fn database(&self) -> Database {
        self.db.clone()
    }
}

/// 全局 MongoDB 句柄槽位
///
/// 通过 `OnceLock` 确保全局只初始化一次，内部用 `Mutex<Option<_>>`
/// 以便在优雅退出时可以 `take()` 掉并触发 Drop 来释放连接。
static MONGO_MANAGER: OnceLock<Mutex<Option<Arc<MongoManager>>>> = OnceLock::new();

fn mongo_slot() -> &'static Mutex<Option<Arc<MongoManager>>> {
    MONGO_MANAGER.get_or_init(|| Mutex::new(None))
}

/// 使用全局配置初始化 MongoDB 单例。
///
/// - 使用 `ConfigEntity` 中的 `mongo_url` / `mongo_database` / `mongo_app` / 用户名密码
/// - 如果已经初始化过，则直接返回已存在的实例
pub async fn init_from_config() -> Result<Arc<MongoManager>, mongodb::error::Error> {
    let cfg = get_or_init_config().clone();

    // 解析连接串
    let mut client_options = ClientOptions::parse(&cfg.mongo_url).await?;

    // 设置 appName，便于在 MongoDB 端区分客户端
    if let Some(app) = cfg.mongo_app.clone() {
        client_options.app_name = Some(app);
    }

    // 如果同时配置了用户名和密码，则构造 Credential
    if let (Some(username), Some(password)) =
        (cfg.mongo_username.clone(), cfg.mongo_password.clone())
    {
        client_options.credential = Some(
            Credential::builder()
                .username(username)
                .password(password)
                .build(),
        );
    }

    // 构建 Client
    let client = Client::with_options(client_options)?;
    let db = client.database(&cfg.mongo_database);

    // 做一次简单的 ping，尽早发现连接问题
    db.run_command(doc! { "ping": 1 }).await?;

    let manager = Arc::new(MongoManager { client, db });

    let slot = mongo_slot();
    let mut guard = slot.lock().expect("lock mongo manager slot");

    if let Some(existing) = guard.as_ref() {
        // 已存在实例，直接复用
        return Ok(existing.clone());
    }

    *guard = Some(manager.clone());
    Ok(manager)
}

/// 获取已经初始化的 MongoDB 单例。
///
/// - 若尚未初始化，返回 `None`
/// - 一般在 `main` 中调用 `init_from_config` 之后，再在业务代码中调用此函数。
pub fn get_manager() -> Option<Arc<MongoManager>> {
    let slot = mongo_slot();
    let guard = slot.lock().expect("lock mongo manager slot");
    guard.clone()
}

/// 便捷函数：直接获取默认 `Database` 句柄。
///
/// - 若尚未初始化，返回 `None`
pub fn get_database() -> Option<Database> {
    get_manager().map(|m| m.database())
}

/// 优雅关闭时调用，用于释放全局 MongoDB 句柄。
///
/// - 将内部的 `Option<Arc<MongoManager>>` 置为 `None`，
///   这样当最后一个 `Arc` 引用被丢弃时，底层连接池会被回收。
/// - 当前实现为幂等，多次调用不会出错。
pub async fn shutdown() {
    let slot = mongo_slot();
    let mut guard = slot.lock().expect("lock mongo manager slot");
    if guard.is_some() {
        // 通过 take 丢弃全局引用，让连接在 Drop 时被释放
        *guard = None;
        println!("MongoDB connection pool has been released.");
    }
}


