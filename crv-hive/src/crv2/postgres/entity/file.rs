use sea_orm::entity::prelude::*;

/// Represents a depot file path that has ever been submitted.
/// One row per unique depot path regardless of how many times it has been
/// deleted and re-created (that lifecycle is tracked via `generation` in
/// `FileRevision`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    /// Canonical depot path, e.g. `//depot/main/src/foo.rs`.
    #[sea_orm(primary_key, auto_increment = false)]
    pub path: String,

    /// Unix timestamp in milliseconds of the very first submission of this path.
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::file_revision::Entity")]
    FileRevision,
}

impl Related<super::file_revision::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FileRevision.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
