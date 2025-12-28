use serde::{Deserialize, Serialize};

/// `users` 集合
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDoc {
    pub id: String,
    /// 用户的明文/加密密码
    pub password: String,
}

/// `branches` 集合中 `metadata` 字段
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchMetadata {
    /// 分支描述
    pub description: String,
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

/// `changelists` 集合中 `changes` 数组的元素
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelistChange {
    /// 受影响的文件 ID（对应 `files` 集合中的 `_id`）
    pub file: String,
    /// 操作类型：create / modify / delete
    pub action: ChangelistAction,
    /// 本次变更对应的 fileRevision ID
    pub revision: String,
}

/// Changelist 中的单条变更动作
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangelistAction {
    Create,
    Modify,
    Delete,
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
    /// 本次 changelist 中包含的变更列表
    pub changes: Vec<ChangelistChange>,
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
    /// 该文件存在于哪些分支，"" 代表在默认分支里存在
    pub seen_on_branches: Vec<String>,
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

// 工作区管理不是通用逻辑，现已从 core 中移除，请 Edge 在自己的逻辑中定义， 后续 Hive 中会定义用于交换 checkout 和 lock 信息的 gRPC 接口