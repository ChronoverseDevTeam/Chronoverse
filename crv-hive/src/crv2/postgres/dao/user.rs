use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, Set};
use sea_orm::ActiveValue::Unchanged;

use crate::crv2::postgres::entity::user::{ActiveModel, Entity, Model};
use super::DaoResult;

/// Input for creating a new user.
pub struct NewUser {
    pub username: String,
    /// Must already be an Argon2 hash — never store plaintext.
    pub password_hash: String,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

/// Look up a user by their username. Returns `None` if not found.
pub async fn find_by_username(
    db: &impl ConnectionTrait,
    username: &str,
) -> DaoResult<Option<Model>> {
    Ok(Entity::find_by_id(username.to_owned()).one(db).await?)
}

/// Insert a new user record. Fails if the username already exists.
pub async fn insert(db: &impl ConnectionTrait, new_user: NewUser) -> DaoResult<()> {
    let am = ActiveModel {
        username: Set(new_user.username),
        password_hash: Set(new_user.password_hash),
        created_at: Set(new_user.created_at),
    };
    am.insert(db).await?;
    Ok(())
}

/// Replace the stored password hash for `username`.
/// Only issues an UPDATE for the `password_hash` column.
pub async fn update_password(
    db: &impl ConnectionTrait,
    username: &str,
    new_hash: &str,
) -> DaoResult<()> {
    let am = ActiveModel {
        username: Unchanged(username.to_owned()),
        password_hash: Set(new_hash.to_owned()),
        ..Default::default()
    };
    am.update(db).await?;
    Ok(())
}

/// Delete a user record by username. Returns `true` if a row was removed.
pub async fn delete(db: &impl ConnectionTrait, username: &str) -> DaoResult<bool> {
    let result = Entity::delete_by_id(username.to_owned()).exec(db).await?;
    Ok(result.rows_affected > 0)
}
