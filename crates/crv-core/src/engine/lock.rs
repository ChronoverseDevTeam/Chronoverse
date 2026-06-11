use crv_shared::error::{CrvError, Result};
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

/// Acquire an exclusive lock on a depot file.
/// Returns an error if the file is already locked by another user.
pub async fn lock_file(
    pool: &PgPool,
    depot_path: &str,
    client_id: Uuid,
    user_id: Uuid,
) -> Result<()> {
    // Check if already locked
    let existing = sqlx::query(
        "SELECT user_id, lock_type FROM locks WHERE depot_path = $1",
    )
    .bind(depot_path)
    .fetch_optional(pool)
    .await
    .map_err(|e| CrvError::Database(format!("lock lookup failed: {e}")))?;

    if let Some(row) = existing {
        let owner: Uuid = row.get("user_id");
        let lt: String = row.get("lock_type");
        if owner != user_id {
            return Err(CrvError::FileLocked {
                user: owner.to_string(),
                client: "unknown".into(),
            });
        }
        if lt == "exclusive" {
            return Err(CrvError::InvalidInput(format!(
                "'{depot_path}' is already locked by you (exclusive)"
            )));
        }
    }

    sqlx::query(
        "INSERT INTO locks (depot_path, client_id, user_id, lock_type)
         VALUES ($1, $2, $3, 'exclusive')
         ON CONFLICT (depot_path) DO UPDATE SET client_id = $2, user_id = $3, lock_type = 'exclusive'",
    )
    .bind(depot_path)
    .bind(client_id)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|e| CrvError::Database(format!("lock insert failed: {e}")))?;

    tracing::info!("File '{depot_path}' locked by user {user_id}");
    Ok(())
}

/// Release a lock on a depot file.
/// Only the lock owner (or an admin) can unlock.
pub async fn unlock_file(
    pool: &PgPool,
    depot_path: &str,
    user_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query(
        "DELETE FROM locks WHERE depot_path = $1 AND user_id = $2",
    )
    .bind(depot_path)
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|e| CrvError::Database(format!("unlock failed: {e}")))?;

    if result.rows_affected() > 0 {
        tracing::info!("File '{depot_path}' unlocked by user {user_id}");
        Ok(true)
    } else {
        // Check if the lock exists but belongs to someone else
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM locks WHERE depot_path = $1)",
        )
        .bind(depot_path)
        .fetch_one(pool)
        .await
        .map_err(|e| CrvError::Database(format!("lock check failed: {e}")))?;

        if exists {
            Err(CrvError::InvalidInput(format!(
                "'{depot_path}' is locked by another user"
            )))
        } else {
            Ok(false) // not locked
        }
    }
}

/// List all current locks, optionally filtered by path pattern.
pub async fn list_locks(
    pool: &PgPool,
    filespec: Option<&str>,
) -> Result<Vec<LockInfo>> {
    let rows = if let Some(pattern) = filespec {
        let like_pattern = pattern.replace("...", "%");
        sqlx::query(
            "SELECT l.depot_path, l.client_id, l.user_id, l.lock_type, l.created_at, u.name as user_name
             FROM locks l JOIN users u ON l.user_id = u.id
             WHERE l.depot_path LIKE $1
             ORDER BY l.depot_path",
        )
        .bind(&like_pattern)
        .fetch_all(pool)
        .await
        .map_err(|e| CrvError::Database(format!("lock list failed: {e}")))?
    } else {
        sqlx::query(
            "SELECT l.depot_path, l.client_id, l.user_id, l.lock_type, l.created_at, u.name as user_name
             FROM locks l JOIN users u ON l.user_id = u.id
             ORDER BY l.depot_path",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| CrvError::Database(format!("lock list failed: {e}")))?
    };

    Ok(rows
        .iter()
        .map(|r| LockInfo {
            depot_path: r.get("depot_path"),
            client_id: r.get("client_id"),
            user_id: r.get("user_id"),
            user_name: r.get("user_name"),
            lock_type: r.get("lock_type"),
            created_at: r.get("created_at"),
        })
        .collect())
}

/// Check if any of the given files are locked by another user.
/// Returns the first conflicting lock found.
pub async fn check_locks(
    pool: &PgPool,
    depot_paths: &[String],
    user_id: Uuid,
) -> Result<()> {
    for path in depot_paths {
        let row = sqlx::query(
            "SELECT user_id FROM locks WHERE depot_path = $1 AND user_id != $2",
        )
        .bind(path)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| CrvError::Database(format!("lock check failed: {e}")))?;

        if let Some(r) = row {
            let owner: Uuid = r.get("user_id");
            return Err(CrvError::FileLocked {
                user: owner.to_string(),
                client: "unknown".into(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct LockInfo {
    pub depot_path: String,
    pub client_id: Uuid,
    pub user_id: Uuid,
    pub user_name: String,
    pub lock_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
