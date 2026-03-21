use sea_orm::entity::prelude::*;

/// A single revision of a depot file within a changelist.
///
/// Primary key is the triple `(path, generation, revision)`.
///
/// - `generation` starts at 1 and increments each time the file is deleted
///   and then re-submitted at the same depot path.
/// - `revision` starts at 1 within each generation and increments with every
///   subsequent submission of that file.
/// - A deletion is represented by `is_deletion = true` and an empty
///   `chunk_hashes` array.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "file_revisions")]
pub struct Model {
    /// Depot path — FK → files.path.
    #[sea_orm(primary_key, auto_increment = false)]
    pub path: String,

    /// File generation counter (increments on delete + re-create).
    #[sea_orm(primary_key, auto_increment = false)]
    pub generation: i64,

    /// Revision number within this generation.
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: i64,

    /// The changelist this revision belongs to — FK → changelists.id.
    pub changelist_id: i64,

    /// Ordered list of content-addressable chunk hashes that compose this
    /// revision's content.  Stored as a JSON array of strings.
    /// Empty when `is_deletion` is `true`.
    pub chunk_hashes: Json,

    /// Total uncompressed file size in bytes (`0` for a deletion).
    pub size: i64,

    /// `true` when this revision marks the file as deleted.
    pub is_deletion: bool,

    /// Unix timestamp in milliseconds of when this revision was created.
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::file::Entity",
        from = "Column::Path",
        to = "super::file::Column::Path"
    )]
    File,

    #[sea_orm(
        belongs_to = "super::changelist::Entity",
        from = "Column::ChangelistId",
        to = "super::changelist::Column::Id"
    )]
    Changelist,
}

impl Related<super::file::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl Related<super::changelist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Changelist.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// Returns the chunk hashes as an owned `Vec<String>`.
    pub fn chunk_hash_list(&self) -> Vec<String> {
        self.chunk_hashes
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}
