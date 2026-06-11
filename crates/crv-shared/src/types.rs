use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── User ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub full_name: Option<String>,
    pub user_type: UserType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UserType {
    #[default]
    Standard,
    Operator,
    Service,
}

// ── Auth ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub ticket: String,
    pub user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthTicket {
    pub user_id: Uuid,
    pub ticket_hash: String,
    pub expires_at: DateTime<Utc>,
    pub client_addr: Option<String>,
}

// ── Client (Workspace) ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSpec {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub root: String,
    pub view: Vec<ViewMapping>,
    pub options: ClientOptions,
    pub host: Option<String>,
    pub stream_id: Option<Uuid>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewMapping {
    pub depot_path: String,
    pub local_path: String,
    pub mapping_type: ViewMappingType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewMappingType {
    Overlay,
    Include,
    Exclude,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientOptions {
    pub allwrite: bool,
    pub clobber: bool,
    pub compress: bool,
    pub locked: bool,
    pub modtime: bool,
    pub rmdir: bool,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            allwrite: false,
            clobber: true,
            compress: false,
            locked: false,
            modtime: false,
            rmdir: false,
        }
    }
}

// ── Group ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub members: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ── Changelist ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeList {
    pub id: Uuid,
    pub number: i64,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub description: String,
    pub status: ChangeListStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeListStatus {
    Pending,
    Submitted,
    Shelved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeListCreate {
    pub description: String,
    pub files: Vec<FileAction>,
}

// ── File ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepotFile {
    pub depot_path: String,
    pub file_type: FileType,
    pub head_revision: i32,
    pub head_action: FileActionType,
    pub head_change: i64,
    pub head_time: DateTime<Utc>,
    pub file_size: i64,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRevision {
    pub id: Uuid,
    pub depot_path: String,
    pub revision: i32,
    pub change_id: Uuid,
    pub action: FileActionType,
    pub file_type: FileType,
    pub digest: String,
    pub size: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAction {
    pub depot_path: String,
    pub action: FileActionType,
    pub file_type: Option<FileType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileActionType {
    Add,
    Edit,
    Delete,
    Branch,
    Integrate,
    MoveAdd,
    MoveDelete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FileType {
    Text,
    Binary,
    Symlink,
    #[serde(rename = "text+x")]
    TextX {
        #[serde(default = "default_exec")]
        exec: bool,
    },
    #[serde(rename = "binary+x")]
    BinaryX {
        #[serde(default = "default_exec")]
        exec: bool,
    },
    #[serde(rename = "unicode")]
    Unicode,
    #[serde(rename = "utf16")]
    Utf16,
}

fn default_exec() -> bool {
    true
}

// ── Have List ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaveEntry {
    pub client_id: Uuid,
    pub depot_path: String,
    pub revision: i32,
    pub digest: String,
    pub file_size: i64,
    pub sync_time: DateTime<Utc>,
}

// ── Lock ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileLock {
    pub depot_path: String,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub lock_type: LockType,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LockType {
    /// Exclusive lock — no other user can submit to this file
    Exclusive,
    /// Shared lock — multiple users can lock, none can submit
    Shared,
}

// ── Branch ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchSpec {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub description: Option<String>,
    pub view: Vec<ViewMapping>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationRecord {
    pub id: Uuid,
    pub source_path: String,
    pub source_start_rev: i32,
    pub source_end_rev: i32,
    pub target_path: String,
    pub target_start_rev: i32,
    pub target_end_rev: i32,
    pub action: IntegrationAction,
    pub change_id: Uuid,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationAction {
    BranchFrom,
    MergeFrom,
    CopyFrom,
    DeleteFrom,
    Ignore,
}

// ── Label ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelSpec {
    pub id: Uuid,
    pub name: String,
    pub owner_id: Uuid,
    pub description: Option<String>,
    pub view: Option<Vec<ViewMapping>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelRevision {
    pub label_id: Uuid,
    pub depot_path: String,
    pub revision: i32,
}

// ── Protection ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionEntry {
    pub id: Uuid,
    pub perm_type: PermType,
    pub perm_level: PermLevel,
    pub entity_type: EntityType,
    pub entity_name: String,
    pub depot_path_pattern: String,
    pub order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PermType {
    Read,
    Write,
    Open,
    Admin,
    Super,
    Review,
    Owner,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PermLevel {
    User,
    Group,
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    User,
    Group,
}

// ── Stream ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSpec {
    pub id: Uuid,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub stream_type: StreamType,
    pub view: Vec<ViewMapping>,
    pub options: StreamOptions,
    pub owner_id: Uuid,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StreamType {
    Mainline,
    Release,
    Development,
    Task,
    Virtual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    pub locked: bool,
    /// Parent view is automatically imported
    pub parent_view_auto: bool,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            locked: false,
            parent_view_auto: true,
        }
    }
}

// ── Sync ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub filespec: Option<String>,
    pub force: bool,
    pub no_update: bool,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub files: Vec<SyncFileEntry>,
    pub total_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFileEntry {
    pub depot_path: String,
    pub revision: i32,
    pub action: FileActionType,
    pub file_type: FileType,
    pub file_size: i64,
    pub digest: String,
    /// Whether this file content needs to be fetched
    pub needs_content: bool,
}

// ── API Response Wrapper ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

// ── CRUD Request Types ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub email: String,
    pub password: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub user_type: UserType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub password: Option<String>,
    pub full_name: Option<String>,
    #[serde(default)]
    pub user_type: Option<UserType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub members: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateGroupRequest {
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifyGroupMembersRequest {
    /// User IDs to add
    #[serde(default)]
    pub add: Vec<Uuid>,
    /// User IDs to remove
    #[serde(default)]
    pub remove: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClientRequest {
    pub name: String,
    pub root: String,
    #[serde(default)]
    pub view: Vec<ViewMapping>,
    #[serde(default)]
    pub options: ClientOptions,
    pub host: Option<String>,
    pub stream_id: Option<Uuid>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateClientRequest {
    pub root: Option<String>,
    pub view: Option<Vec<ViewMapping>>,
    pub options: Option<ClientOptions>,
    pub host: Option<String>,
    pub stream_id: Option<Uuid>,
    pub description: Option<String>,
}
