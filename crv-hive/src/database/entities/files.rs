use sea_orm::entity::prelude::*;
use super::file_revisions;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub path: String,
    pub created_at: i64,
    pub metadata: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "file_revisions::Entity")]
    FileRevisions,
}

impl Related<file_revisions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FileRevisions.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}


