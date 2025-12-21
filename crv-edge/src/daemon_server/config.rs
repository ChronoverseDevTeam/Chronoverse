//! 配置相关的结构。

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::{
    daemon_server::error::{AppError, AppResult},
    pb,
};

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
    pub const CONFY_APP_NAME: &'static str = "crv-edge";
    pub const CONFY_CONFIG_NAME: &'static str = "bootstrap";

    /// 计算默认数据目录
    fn get_default_data_dir() -> String {
        // 使用 ProjectDirs 获取跨平台的路径
        if let Some(proj_dirs) = ProjectDirs::from("com", "ChronoverseDevTeam", "crv-edge") {
            proj_dirs.data_local_dir().to_string_lossy().to_string()
        } else {
            // 如果 ProjectDirs 无法确定路径，提供一个最后的回退方案
            #[cfg(not(windows))]
            return String::from("/tmp/chronoverse_data_fallback");
            #[cfg(windows)]
            return String::from("C:/chronoverse_data_fallback");
        }
    }

    pub fn load() -> AppResult<Self> {
        let config = confy::load::<Self>(Self::CONFY_APP_NAME, Self::CONFY_CONFIG_NAME)
            .map_err(|e| AppError::Config(format!("{e}")))?;
        Ok(config)
    }
}

/// daemon 运行时所需的配置项。
#[derive(Clone)]
pub struct RuntimeConfig {
    /// hive 地址
    pub remote_addr: RuntimeConfigItem,
    /// 启动文本编辑器的指令
    pub editor: RuntimeConfigItem,
    /// 当前用户
    pub user: RuntimeConfigItem,
}

#[derive(Clone)]
pub struct RuntimeConfigItem {
    pub value: String,
    pub source: RuntimeConfigSource,
}

#[derive(Clone, Copy)]
pub enum RuntimeConfigSource {
    Default,
    Set,
    Override,
}

impl Into<String> for RuntimeConfigSource {
    fn into(self) -> String {
        match self {
            RuntimeConfigSource::Default => "default".to_string(),
            RuntimeConfigSource::Set => "set".to_string(),
            RuntimeConfigSource::Override => "override".to_string(),
        }
    }
}

impl RuntimeConfig {
    /// 合并逻辑：将 override 应用到 self 上
    pub fn merge(&mut self, other: RuntimeConfigOverride, source: RuntimeConfigSource) {
        if let Some(value) = other.remote_addr {
            self.remote_addr = RuntimeConfigItem { value, source };
        }
        if let Some(value) = other.editor {
            self.editor = RuntimeConfigItem { value, source };
        }
        if let Some(value) = other.user {
            self.user = RuntimeConfigItem { value, source };
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            remote_addr: RuntimeConfigItem {
                value: "localhost:34560".to_string(),
                source: RuntimeConfigSource::Default,
            },
            editor: RuntimeConfigItem {
                value: "vim".to_string(),
                source: RuntimeConfigSource::Default,
            },
            user: RuntimeConfigItem {
                value: "default".to_string(),
                source: RuntimeConfigSource::Default,
            },
        }
    }
}

impl Into<pb::RuntimeConfigItem> for RuntimeConfigItem {
    fn into(self) -> pb::RuntimeConfigItem {
        pb::RuntimeConfigItem {
            value: self.value,
            source: self.source.into(),
        }
    }
}

/// 从用户请求的元数据中提取的用于覆盖运行时配置的信息。
#[derive(Serialize, Deserialize)]
pub struct RuntimeConfigOverride {
    pub remote_addr: Option<String>,
    pub editor: Option<String>,
    pub user: Option<String>,
}
