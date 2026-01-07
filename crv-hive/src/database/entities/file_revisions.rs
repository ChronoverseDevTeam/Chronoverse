use sea_orm::entity::prelude::*;
use super::files;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_revisions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub path: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub generation: u64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: u64,
    pub changelist_id: i64,
    pub binary_id: Json,
    pub size: u64,
    pub is_delete: bool,
    pub created_at: i64,
    pub metadata: Json,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "files::Entity",
        from = "Column::Path",
        to = "files::Column::Path"
    )]
    File,
}

impl Related<files::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::File.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}


