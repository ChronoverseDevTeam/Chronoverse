use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::config::entity::ConfigEntity;

static CONFIG: OnceLock<ConfigEntity> = OnceLock::new();

fn default_config_path() -> PathBuf {
    // 优先使用环境变量 CRV_HIVE_CONFIG 指定的路径，否则使用工作目录下的 hive.toml
    if let Ok(p) = env::var("CRV_HIVE_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    PathBuf::from("hive.toml")
}

fn ensure_parent_dir(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

pub fn get_config() -> Option<&'static ConfigEntity> {
    CONFIG.get()
}

pub fn get_or_init_config() -> &'static ConfigEntity {
    CONFIG.get_or_init(|| ConfigEntity::default())
}

/// 在首次初始化前注入配置（例如覆盖 Postgres 的 host/port）。
///
/// - 成功：返回 `Ok(())`
/// - 若已初始化：返回 `Err`
pub fn try_set_config(cfg: ConfigEntity) -> Result<(), &'static str> {
    CONFIG.set(cfg).map_err(|_| "config already initialized")
}

pub async fn load_config() -> Result<(), Box<dyn std::error::Error>> {
    let path = default_config_path();
    if path.exists() {
        let content = tokio::fs::read_to_string(&path).await?;
        let cfg: ConfigEntity = toml::from_str(&content)?;
        let _ = CONFIG.set(cfg);
    } else {
        let cfg = ConfigEntity::default();
        let toml_str = toml::to_string_pretty(&cfg)?;
        ensure_parent_dir(&path)?;
        tokio::fs::write(&path, toml_str).await?;
        let _ = CONFIG.set(cfg);
    }
    Ok(())
}

pub async fn save_config() -> Result<(), Box<dyn std::error::Error>> {
    let path = default_config_path();
    if let Some(cfg) = CONFIG.get() {
        let toml_str = toml::to_string_pretty(cfg)?;
        ensure_parent_dir(&path)?;
        tokio::fs::write(&path, toml_str).await?;
        Ok(())
    } else {
        let cfg = ConfigEntity::default();
        let toml_str = toml::to_string_pretty(&cfg)?;
        ensure_parent_dir(&path)?;
        tokio::fs::write(&path, toml_str).await?;
        let _ = CONFIG.set(cfg);
        Ok(())
    }
}

/// 优雅关闭时调用的配置持久化钩子（如果需要将内存更改落盘）。
pub async fn shutdown_config() -> Result<(), Box<dyn std::error::Error>> {
    // 当前仅将内存中的 CONFIG 再次保存一次，确保外部可能的变更写回。
    // 若无变更也不会有副作用。
    save_config().await
}
