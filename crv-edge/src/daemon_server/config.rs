//! 配置相关的结构。

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::daemon_server::error::{AppError, AppResult};

/// daemon 启动时所需的配置项。
#[derive(Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// daemon 启动时的端口号
    pub daemon_port: u16,
    /// 嵌入式数据库存放数据的根目录
    pub embedded_database_root: String,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            daemon_port: 31822,
            embedded_database_root: Self::get_default_data_dir(),
        }
    }
}

impl BootstrapConfig {
    /// 计算默认数据目录
    fn get_default_data_dir() -> String {
        // 使用 ProjectDirs 获取跨平台的路径
        if let Some(proj_dirs) = ProjectDirs::from("com", "ChronoverseDevTeam", "crv-edge") {
            proj_dirs.data_local_dir().to_string_lossy().to_string()
        } else {
            // 如果 ProjectDirs 无法确定路径，提供一个最后的回退方案
            #[cfg(windows)]
            return String::from("/tmp/chronoverse_data_fallback");
            #[cfg(not(windows))]
            return String::from("C:/chronoverse_data_fallback");
        }
    }

    pub fn load() -> AppResult<Self> {
        let config = confy::load::<Self>("crv-edge", "bootstrap")
            .map_err(|e| AppError::Config(format!("{e}")))?;
        Ok(config)
    }
}

/// daemon 运行时所需的配置项。
#[derive(Clone)]
pub struct RuntimeConfig {
    /// hive 地址
    pub remote_addr: String,
    /// 启动文本编辑器的指令
    pub editor: String,
}

impl RuntimeConfig {
    /// 合并逻辑：将 override 应用到 self 上
    pub fn merge(&mut self, other: RuntimeConfigOverride) {
        if let Some(v) = other.remote_addr {
            self.remote_addr = v;
        }
        if let Some(v) = other.editor {
            self.editor = v;
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            remote_addr: "localhost:31823".to_string(),
            editor: "vim".to_string(),
        }
    }
}

/// 从用户请求的元数据中提取的用于覆盖运行时配置的信息。
#[derive(Serialize, Deserialize)]
pub struct RuntimeConfigOverride {
    pub remote_addr: Option<String>,
    pub editor: Option<String>,
}
