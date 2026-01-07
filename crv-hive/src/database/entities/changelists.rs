use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "changelists")]
pub struct Model {
    // 与 proto / 业务逻辑保持一致：changelist_id 为 int64。
    // Postgres 无 unsigned bigint，使用 i64 更稳妥。
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i64,
    pub author: String,
    pub description: String,
    pub changes: Json,
    pub committed_at: i64,
    pub metadata: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
