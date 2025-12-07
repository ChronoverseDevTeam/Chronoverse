//! 服务全局状态管理
use super::db::DbManager;
use std::sync::Arc;

/// 全局应用状态，将被注入到 gRPC Service 中
#[derive(Clone)]
pub struct AppState {
    /// 数据库管理器
    pub db: Arc<DbManager>,
    // 如果有连接中心化服务的客户端，也可以放在这
    // pub hive_client: Option<Arc<HiveClient>>,
}

impl AppState {
    pub fn new(db: Arc<DbManager>) -> Self {
        Self { db }
    }
}
