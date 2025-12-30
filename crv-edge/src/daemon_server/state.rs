//! 服务全局状态管理
use crate::daemon_server::error::{AppError, AppResult};

use super::db::DbManager;
use lru::LruCache;
use std::{num::NonZeroUsize, sync::Arc};
use tonic::transport::{Channel, Endpoint};

/// 全局应用状态，将被注入到 gRPC Service 中
#[derive(Clone)]
pub struct AppState {
    /// 数据库管理器
    pub db: Arc<DbManager>,
    /// 与 hive 的连接通道池
    pub hive_channel: Arc<ChannelPool>,
}

/// 缓存连接
pub struct ChannelPool {
    channel_cache: Arc<std::sync::Mutex<LruCache<String, Channel>>>,
}

impl ChannelPool {
    const CACHE_CAPACITY: usize = 64;

    pub fn new() -> Self {
        Self {
            channel_cache: Arc::new(std::sync::Mutex::new(LruCache::new(
                NonZeroUsize::new(Self::CACHE_CAPACITY).unwrap(),
            ))),
        }
    }

    pub fn get_channel(&self, addr: &str) -> AppResult<Channel> {
        let mut cache = self
            .channel_cache
            .lock()
            .map_err(|e| AppError::Internal(format!("{e}")))?;

        if let Some(channel) = cache.get(addr) {
            return Ok(channel.clone());
        }

        drop(cache);

        let channel = Endpoint::from_shared(addr.to_string())
            .map_err(|e| AppError::Internal(format!("{e}")))?
            .connect_lazy();

        let mut cache = self
            .channel_cache
            .lock()
            .map_err(|e| AppError::Internal(format!("{e}")))?;

        cache.put(addr.to_string(), channel.clone());

        return Ok(channel);
    }
}

impl AppState {
    pub fn new(db: Arc<DbManager>) -> Self {
        Self {
            db,
            hive_channel: Arc::new(ChannelPool::new()),
        }
    }
}
