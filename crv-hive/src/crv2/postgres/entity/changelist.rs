use sea_orm::entity::prelude::*;

/// A submitted changelist (analogous to a P4 changelist or a git commit).
/// Groups one or more file revisions into a single atomic submission.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "changelists")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,

    /// Username of the submitter.
    pub author: String,

    /// Free-form description provided at submit time.
    pub description: String,

    /// Unix timestamp in milliseconds of when this changelist was committed.
    pub committed_at: i64,
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
