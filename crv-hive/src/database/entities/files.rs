use sea_orm::entity::prelude::*;
use sea_orm::Set;
use super::file_revisions;
use crate::database::ltree_key;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "files")]
pub struct Model {
    /// Postgres `ltree` 主键。
    ///
    /// 注意：这不是原始 depot path 字符串（`//a/b/c.txt`），而是经过编码后的 ltree key。
    /// 编码规则见 `crate::database::ltree_key`。
    #[sea_orm(primary_key, auto_increment = false, column_type = "custom(\"ltree\")")]
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

impl Model {
    /// 将数据库里的 `ltree key` 反解码为原始 depot path（形如 `//a/b/c.txt`）。
    pub fn to_depot_path_string(&self) -> Result<String, ltree_key::LtreeKeyError> {
        ltree_key::ltree_key_to_depot_path_str(&self.path)
    }
}

impl ActiveModel {
    /// 便捷构造：用原始 depot path（形如 `//a/b/c.txt`）创建 `files` 的 ActiveModel，
    /// 并自动编码为 `ltree key`。
    pub fn from_depot_path_str(
        depot_path: &str,
        created_at: i64,
        metadata: Json,
    ) -> Result<Self, ltree_key::LtreeKeyError> {
        Ok(Self {
            path: Set(ltree_key::depot_path_str_to_ltree_key(depot_path)?),
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