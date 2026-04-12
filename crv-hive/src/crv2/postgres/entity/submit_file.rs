use sea_orm::entity::prelude::*;

/// Action to be performed on a file within a submit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileAction {
    Add,
    Edit,
    Delete,
}

impl FileAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Edit => "edit",
            Self::Delete => "delete",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "add" => Some(Self::Add),
            "edit" => Some(Self::Edit),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}

/// A single file entry within a submit.
///
/// While the parent submit is `pending`, this row acts as a pessimistic
/// lock on the depot path — any attempt to include the same path in
/// another pending submit will be rejected.
///
/// `chunk_hashes` and `size` describe the new content to be committed.
/// For deletions, `chunk_hashes` is an empty JSON array and `size` is 0.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "submit_files")]
pub struct Model {
    /// FK → submits.id.
    #[sea_orm(primary_key, auto_increment = false)]
    pub submit_id: i64,

    /// Depot path of the file being submitted.
    #[sea_orm(primary_key, auto_increment = false)]
    pub path: String,

    /// Action: add | edit | delete.
    pub action: String,

    /// Ordered list of BLAKE3 chunk hashes (hex strings) composing the new
    /// file content.  Empty JSON array for deletions.
    pub chunk_hashes: Json,

    /// Total uncompressed file size in bytes (0 for deletions).
    pub size: i64,
}

impl Model {
    pub fn parsed_action(&self) -> Option<FileAction> {
        FileAction::from_str(&self.action)
    }

    /// Returns the chunk hashes as an owned `Vec<String>`.
    pub fn chunk_hash_list(&self) -> Vec<String> {
        self.chunk_hashes
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::submit::Entity",
        from = "Column::SubmitId",
        to = "super::submit::Column::Id"
    )]
    Submit,

    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::Path",
        to = "super::file::Column::Path"
    )]
    File,
}

impl Related<super::submit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Submit.def()
    }
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
