use crate::path::basic::{DepotPath, DepotPathWildcard, LocalDir, LocalPath, RangeDepotWildcard};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntity {
    #[serde(rename = "_id")]
    pub name: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
    pub owner: String,
    pub path: String,
    pub device_finger_print: String,
}

/// Workspace 映射关系
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceMapping {
    Include(IncludeMapping),
    Exclude(ExcludeMapping),
}

/// Workspace 包含映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IncludeMapping {
    File(FileMapping),
    Range(FolderMapping),
}

/// Workspace 排除映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExcludeMapping {
    File(DepotPath),
    Range(DepotPathWildcard),
}

/// 单文件映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMapping {
    /// Depot 文件
    pub depot_file: DepotPath,
    /// 本地文件
    pub local_file: LocalPath,
}

/// 文件夹映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMapping {
    /// Depot 路径范围
    pub depot_folder: RangeDepotWildcard,
    /// 本地文件夹
    pub local_folder: LocalDir,
}

/// Workspace 配置 TODO: 需要接到 WorkspaceEntity 中
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// 根目录
    pub root_dir: LocalDir,
    /// 映射列表（按顺序处理，后面的覆盖前面的）
    pub mappings: Vec<WorkspaceMapping>,
}
