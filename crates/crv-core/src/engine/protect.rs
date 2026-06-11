use crv_shared::error::{CrvError, Result};
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

/// Evaluate whether a user has the required permission on a depot path.
///
/// Checks the protections table in order. The first matching entry wins.
/// If no entry matches, default is deny for write/open/admin, allow for read.
pub async fn check_permission(
    pool: &PgPool,
    user_id: Uuid,
    depot_path: &str,
    required: &str, // "read", "write", "open", "admin", "super"
) -> Result<bool> {
    if required == "super" {
        // Only superusers pass
        let is_super: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND user_type = 'super')",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| CrvError::Database(format!("perm check: {e}")))?;
        return Ok(is_super);
    }

    // Get user's group memberships
    let groups: Vec<String> = sqlx::query_scalar(
        "SELECT g.name FROM groups g
         JOIN group_members gm ON g.id = gm.group_id
         WHERE gm.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("perm check groups: {e}")))?;

    // Get user name
    let user_name: String = sqlx::query_scalar("SELECT name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| CrvError::Database(format!("perm check user: {e}")))?;

    // Query protections ordered by priority
    let rows = sqlx::query(
        "SELECT perm_type, perm_level, entity_type, entity_name, depot_path_pattern
         FROM protections ORDER BY \"order\", depot_path_pattern",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("perm query: {e}")))?;

    for row in &rows {
        let perm_type: String = row.get("perm_type");
        let perm_level: String = row.get("perm_level");
        let entity_type: String = row.get("entity_type");
        let entity_name: String = row.get("entity_name");
        let pattern: String = row.get("depot_path_pattern");

        // Check if pattern matches depot_path
        if !path_matches(depot_path, &pattern) {
            continue;
        }

        // Check if this entry applies to the user
        let applies = match (perm_level.as_str(), entity_type.as_str()) {
            ("any", _) => true,
            ("user", "user") => entity_name == user_name,
            ("group", "group") => groups.contains(&entity_name),
            _ => false,
        };

        if !applies {
            continue;
        }

        // Check if the required permission is granted
        let granted = match required {
            "read" => matches!(perm_type.as_str(), "read" | "write" | "open" | "admin" | "super"),
            "write" => matches!(perm_type.as_str(), "write" | "open" | "admin" | "super"),
            "open" => matches!(perm_type.as_str(), "open" | "admin" | "super"),
            "admin" => matches!(perm_type.as_str(), "admin" | "super"),
            _ => false,
        };

        return Ok(granted);
    }

    // Default: read is allowed, everything else denied
    Ok(required == "read")
}

/// Simple glob matching for depot paths.
fn path_matches(path: &str, pattern: &str) -> bool {
    if pattern == "*" || pattern == "//..." {
        return true;
    }
    let pattern = pattern.replace("...", "**");
    // Simple prefix + wildcard matching
    if pattern.contains("**") {
        let prefix = pattern.trim_end_matches("**").trim_end_matches('/');
        path.starts_with(prefix)
    } else if pattern.contains('*') {
        let prefix = pattern.split('*').next().unwrap_or("");
        path.starts_with(prefix)
    } else {
        path == pattern
    }
}

/// List all protection entries.
pub async fn list_protections(pool: &PgPool) -> Result<Vec<ProtectionRow>> {
    let rows = sqlx::query(
        "SELECT id, perm_type, perm_level, entity_type, entity_name, depot_path_pattern, \"order\"
         FROM protections ORDER BY \"order\"",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("protect list: {e}")))?;

    Ok(rows
        .iter()
        .map(|r| ProtectionRow {
            id: r.get("id"),
            perm_type: r.get("perm_type"),
            perm_level: r.get("perm_level"),
            entity_type: r.get("entity_type"),
            entity_name: r.get("entity_name"),
            depot_path_pattern: r.get("depot_path_pattern"),
            order: r.get("order"),
        })
        .collect())
}

/// Add a protection entry.
pub async fn add_protection(
    pool: &PgPool,
    perm_type: &str,
    perm_level: &str,
    entity_type: &str,
    entity_name: &str,
    depot_path_pattern: &str,
    order: i32,
) -> Result<Uuid> {
    let row = sqlx::query(
        "INSERT INTO protections (perm_type, perm_level, entity_type, entity_name, depot_path_pattern, \"order\")
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(perm_type)
    .bind(perm_level)
    .bind(entity_type)
    .bind(entity_name)
    .bind(depot_path_pattern)
    .bind(order)
    .fetch_one(pool)
    .await
    .map_err(|e| CrvError::Database(format!("protect insert: {e}")))?;

    Ok(row.get("id"))
}

/// Delete a protection entry.
pub async fn delete_protection(pool: &PgPool, id: Uuid) -> Result<bool> {
    let r = sqlx::query("DELETE FROM protections WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| CrvError::Database(format!("protect delete: {e}")))?;
    Ok(r.rows_affected() > 0)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProtectionRow {
    pub id: Uuid,
    pub perm_type: String,
    pub perm_level: String,
    pub entity_type: String,
    pub entity_name: String,
    pub depot_path_pattern: String,
    pub order: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_matches() {
        assert!(path_matches("//depot/main/file.txt", "//depot/main/..."));
        assert!(path_matches("//depot/main/src/a.rs", "//depot/main/..."));
        assert!(path_matches("//depot/main/file.txt", "//depot/main/*"));
        assert!(!path_matches("//depot/other/file.txt", "//depot/main/..."));
        assert!(path_matches("//depot/main/file.txt", "//..."));
        assert!(path_matches("//depot/main/file.txt", "*"));
    }
}
