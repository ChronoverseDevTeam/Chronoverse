use serde::{Deserialize, Serialize};

/// `users` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDoc {
    /// Mongo `_id`，即用户名
    #[serde(rename = "_id")]
    pub id: String,
    /// 用户的明文/加密密码
    pub password: String,
}

/// `branches` 集合中 `metadata` 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchMetadata {
    /// 分支描述
    pub description: String,
    /// 分支所有者列表
    pub owners: Vec<String>,
}

/// `branches` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDoc {
    /// Mongo `_id`，内部分支 ID
    #[serde(rename = "_id")]
    pub id: String,
    /// 创建时间（Linux 时间戳，毫秒）
    pub created_at: i64,
    /// 创建人用户名
    pub created_by: String,
    /// 当前 HEAD 指向的 changelist
    pub head_changelist_id: i64,
    /// 附加元信息
    pub metadata: BranchMetadata,
}

/// `changelists` 集合中 `metadata` 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelistMetadata {
    /// 标签列表
    pub labels: Vec<String>,
}

/// `changelists` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelistDoc {
    /// 自增 ID
    #[serde(rename = "_id")]
    pub id: i64,
    pub parent_changelist_id: i64,
    pub branch_id: String,
    pub author: String,
    pub description: String,
    /// 提交时间（Linux 时间戳，毫秒）
    pub committed_at: i64,
    /// 文件数量统计字段
    pub files_count: i64,
    /// 附加元信息
    pub metadata: ChangelistMetadata,
}

/// `files` 集合中 `metadata` 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// 第一次引入该文件的用户
    pub first_introduced_by: String,
}

/// `files` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDoc {
    /// 文件 ID，对路径采用 Blake3 哈希计算得出
    #[serde(rename = "_id")]
    pub id: String,
    /// 规范化路径，例如：`//src/module/a.cpp`
    pub path: String,
    /// 创建时间（Linux 时间戳，毫秒）
    pub created_at: i64,
    /// 附加元信息
    pub metadata: FileMetadata,
}

/// `fileRevision` 集合中 `metadata` 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRevisionMetadata {
    /// 文件权限，例如 `"755"`
    pub file_mode: String,
    /// 内容哈希，用于去重/校验
    pub hash: String,
    /// 是否为二进制文件
    pub is_binary: bool,
    /// 语言，例如 `"cpp"`
    pub language: String,
}

/// `fileRevision` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRevisionDoc {
    /// 对 branch + fileId + changelistId 拼接后进行 Blake3 哈希得到
    #[serde(rename = "_id")]
    pub id: String,
    pub branch_id: String,
    pub file_id: String,
    pub changelist_id: i64,
    /// 指向二进制存储系统的 blob 列表
    pub binary_id: Vec<String>,
    /// 上一个版本，当前设计为单一 parent
    pub parent_revision_id: String,
    /// 文件大小（字节）
    pub size: i64,
    /// 是否为删除操作的 revision
    pub is_delete: bool,
    /// 创建时间（Linux 时间戳，毫秒）
    pub created_at: i64,
    /// 附加元信息
    pub metadata: FileRevisionMetadata,
}

/// Workspace 跟踪条目的状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceTrackingStatus {
    Modified,
    Added,
    Deleted,
}

/// `workspaces` 集合中 `tracking` 数组的元素
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTrackingItem {
    pub file_id: String,
    pub status: WorkspaceTrackingStatus,
}

/// `workspaces` 集合中 `metadata` 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    pub hostname: String,
}

/// `workspaces` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDoc {
    /// Workspace 全局唯一 ID
    #[serde(rename = "_id")]
    pub id: String,
    pub user_id: String,
    pub base_branch_id: String,
    pub base_changelist_id: i64,
    pub tracking: Vec<WorkspaceTrackingItem>,
    /// 创建时间（Linux 时间戳，毫秒）
    pub created_at: i64,
    /// 更新时间（Linux 时间戳，毫秒）
    pub updated_at: i64,
    pub metadata: WorkspaceMetadata,
}

