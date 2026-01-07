use sea_orm::entity::prelude::*;
use sea_orm::Set;
use super::files;
use crate::database::ltree_key;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "file_revisions")]
pub struct Model {
    /// Postgres `ltree` 路径 key（并作为复合主键的一部分）。
    ///
    /// 注意：这不是原始 depot path 字符串（`//a/b/c.txt`），而是经过编码后的 ltree key。
    /// 编码规则见 `crate::database::ltree_key`。
    #[sea_orm(primary_key, auto_increment = false, column_type = "custom(\"ltree\")")]
    pub path: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub generation: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub revision: i64,
    pub changelist_id: i64,
    pub binary_id: Json,
    pub size: i64,
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

impl Model {
    /// 将数据库里的 `ltree key` 反解码为原始 depot path（形如 `//a/b/c.txt`）。
    pub fn to_depot_path_string(&self) -> Result<String, ltree_key::LtreeKeyError> {
        ltree_key::ltree_key_to_depot_path_str(&self.path)
    }
}

impl ActiveModel {
    /// 便捷构造：用原始 depot path（形如 `//a/b/c.txt`）创建 `file_revisions` 的 ActiveModel，
    /// 并自动编码为 `ltree key`。
    pub fn from_depot_path_str(
        depot_path: &str,
        generation: i64,
        revision: i64,
        changelist_id: i64,
        binary_id: Json,
        size: i64,
        is_delete: bool,
        created_at: i64,
        metadata: Json,
    ) -> Result<Self, ltree_key::LtreeKeyError> {
        Ok(Self {
            path: Set(ltree_key::depot_path_str_to_ltree_key(depot_path)?),
            generation: Set(generation),
            revision: Set(revision),
            changelist_id: Set(changelist_id),
            binary_id: Set(binary_id),
            size: Set(size),
            is_delete: Set(is_delete),
            created_at: Set(created_at),
            metadata: Set(metadata),
        })
    }

    /// 将原始 depot path 编码后写入 `path` 字段（`ltree key`）。
    pub fn set_path_from_depot_path_str(
        mut self,
        depot_path: &str,
    ) -> Result<Self, ltree_key::LtreeKeyError> {
        self.path = Set(ltree_key::depot_path_str_to_ltree_key(depot_path)?);
        Ok(self)
    }
}
