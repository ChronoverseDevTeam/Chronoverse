use std::sync::OnceLock;

use crate::{caching::ChunkCache, hive_server::submit::service::SubmitService};

pub static SUBMIT_SERVICE: OnceLock<SubmitService> = OnceLock::new();

/// 获取全局 `SubmitService` 实例。
///
/// - 首次调用时会自动初始化（`SubmitService::new()`）。
/// - 后续调用复用同一实例。
pub fn submit_service() -> &'static SubmitService {
    SUBMIT_SERVICE.get_or_init(SubmitService::new)
}

pub static CACHE_SERVICE: OnceLock<ChunkCache> = OnceLock::new();

pub fn cache_service() -> &'static ChunkCache {
    CACHE_SERVICE.get_or_init(|| {
        ChunkCache::from_config().expect("init ChunkCache from config (repository_path)")
    })
}

pub mod launch_submit;
pub mod submit;
pub mod service;
pub mod upload_file_chunk;