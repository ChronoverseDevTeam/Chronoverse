use crate::daemon_server::config::RuntimeConfigOverride;
use crate::daemon_server::state::AppState;
use tonic::{Request, Status, service::Interceptor};

#[derive(Clone)]
pub struct ConfigInterceptor {
    /// 持有 AppState，从而能够访问 DB
    state: AppState,
}

impl ConfigInterceptor {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl Interceptor for ConfigInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        // 1. 【读持久化配置】 从 RocksDB 读取基础配置
        // 如果 DB IO 很慢，会阻塞 executor 线程，但在本地 Daemon + RocksDB 场景下通常是微秒级，可接受。
        let mut final_config = self
            .state
            .db
            .load_runtime_config()
            .map_err(|e| Status::internal(format!("Failed to load config: {}", e)))?
            .unwrap_or_default(); // 如果 DB 没存，就用默认值

        // 2. 【读临时覆盖】 检查 Metadata (Headers)
        // 假设 CLI 发送请求时，将覆盖参数序列化为 JSON 放在 "x-crv-config-override" 头里
        if let Some(val_bytes) = req.metadata().get("x-crv-config-override") {
            if let Ok(val_str) = val_bytes.to_str() {
                // 解析 JSON
                if let Ok(overrides) = serde_json::from_str::<RuntimeConfigOverride>(val_str) {
                    // 3. 【合并】
                    final_config.merge(overrides);
                } else {
                    // 也可以选择在这里报错，或者仅记录警告忽略格式错误的 override
                    // return Err(Status::invalid_argument("Invalid config override format"));
                }
            }
        }

        // 4. 【注入】 将最终配置放入 Extensions
        req.extensions_mut().insert(final_config);

        Ok(req)
    }
}
