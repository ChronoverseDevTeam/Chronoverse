use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "changelists")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub parent_changelist_id: i64,
    pub branch_id: String,
    pub author: String,
    pub description: String,
    pub changes: Json,
    pub committed_at: i64,
    pub files_count: i64,
    pub metadata: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}


