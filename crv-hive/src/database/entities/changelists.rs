use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "changelists")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: u64,
    pub author: String,
    pub description: String,
    pub changes: Json,
    pub committed_at: i64,
    pub metadata: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}


