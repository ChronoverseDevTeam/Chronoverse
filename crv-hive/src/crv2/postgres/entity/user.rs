use sea_orm::entity::prelude::*;

/// A registered user account.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    /// Unique username, used as the primary key.
    #[sea_orm(primary_key, auto_increment = false)]
    pub username: String,

    /// Argon2 password hash (never stored in plaintext).
    pub password_hash: String,

    /// Unix timestamp in milliseconds of account creation.
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
