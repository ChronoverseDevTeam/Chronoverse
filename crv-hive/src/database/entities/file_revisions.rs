use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_revisions")]
pub struct Model {
    pub file: Model,
    pub generation: u64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: u64,
    pub file_id: String,
    pub changelist_id: i64,
    pub binary_id: Json,
    pub parent_revision_id: String,
    pub size: i64,
    pub is_delete: bool,
    pub created_at: i64,
    pub metadata: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}


