use std::collections::HashMap;

use ::serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::metadata::file_revision::MetaFileRevision;

mod chrono_dt_option_as_bson_dt {
    use chrono::{DateTime, Utc};
    use serde::Serialize as SerdeSerialize;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Map Option<chrono::DateTime> -> Option<bson::DateTime> and delegate to Serde
        let mapped: Option<bson::DateTime> = value.map(bson::DateTime::from_chrono);
        SerdeSerialize::serialize(&mapped, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize as Option<bson::DateTime> then map to Option<chrono::DateTime>
        let opt_bson: Option<bson::DateTime> = Option::<bson::DateTime>::deserialize(deserializer)?;
        Ok(opt_bson.map(|bdt| bdt.to_chrono()))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Changelist {
    #[serde(rename = "_id")]
    pub id: u64,
    pub description: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(
        with = "chrono_dt_option_as_bson_dt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub submitted_at: Option<DateTime<Utc>>,
    pub owner: String,
    pub workspace_name: String,
    // Key 是 depot_path
    pub files: HashMap<String, MetaFileRevision>,
}

impl Changelist {
    /// 创建新的变更列表
    pub fn new(id: u64, owner: String, workspace_name: String, description: String) -> Self {
        Self {
            id,
            description,
            created_at: Utc::now(),
            submitted_at: None,
            owner,
            workspace_name,
            files: HashMap::new(),
        }
    }

    /// 创建默认变更列表（ID 为 0）
    pub fn new_default(owner: String, workspace_name: String) -> Self {
        Self::new(0, owner, workspace_name, "Default changelist".to_string())
    }

    /// 添加文件到变更列表
    pub fn add_file(&mut self, depot_path: String, revision: MetaFileRevision) {
        self.files.insert(depot_path, revision);
    }

    /// 批量添加文件
    pub fn add_files(&mut self, files: Vec<(String, MetaFileRevision)>) {
        for (path, revision) in files {
            self.files.insert(path, revision);
        }
    }

    /// 移除文件
    pub fn remove_file(&mut self, depot_path: &str) -> Option<MetaFileRevision> {
        self.files.remove(depot_path)
    }

    /// 批量移除文件
    pub fn remove_files(&mut self, depot_paths: &[String]) -> Vec<String> {
        let mut removed = Vec::new();
        for path in depot_paths {
            if self.files.remove(path).is_some() {
                removed.push(path.clone());
            }
        }
        removed
    }

    /// 清空所有文件
    pub fn clear_files(&mut self) {
        self.files.clear();
    }

    /// 检查是否包含某个文件
    pub fn contains_file(&self, depot_path: &str) -> bool {
        self.files.contains_key(depot_path)
    }

    /// 获取文件数量
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// 检查是否为空（没有文件）
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// 检查是否已提交
    pub fn is_submitted(&self) -> bool {
        self.submitted_at.is_some()
    }

    /// 检查是否为默认变更列表
    pub fn is_default(&self) -> bool {
        self.id == 0
    }

    /// 标记为已提交
    pub fn mark_submitted(&mut self) {
        self.submitted_at = Some(Utc::now());
    }

    /// 获取所有文件路径
    pub fn file_paths(&self) -> Vec<&String> {
        self.files.keys().collect()
    }

    /// 获取特定文件的版本
    pub fn get_file_revision(&self, depot_path: &str) -> Option<&MetaFileRevision> {
        self.files.get(depot_path)
    }
}
