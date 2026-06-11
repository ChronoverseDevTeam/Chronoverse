use crv_shared::error::{CrvError, Result};
use crv_shared::types::{FileActionType, FileType, SyncFileEntry};
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

use crate::storage::depot::Depot;

/// Compute the list of files a client needs to sync.
///
/// For each file matching the optional filespec, check the depot head revision
/// against the client's have list and determine what needs to be transferred.
pub async fn compute_sync(
    pool: &PgPool,
    _depot: &Depot,
    client_id: Uuid,
    filespec: Option<&str>,
    force: bool,
) -> Result<Vec<SyncFileEntry>> {
    // Fetch depot head revisions
    let depot_files = if let Some(spec) = filespec {
        let pattern = spec.replace("...", "%");
        sqlx::query(
            "SELECT DISTINCT ON (depot_path) depot_path, revision, action, file_type, digest, size
             FROM file_revisions
             WHERE depot_path LIKE $1
             ORDER BY depot_path, revision DESC",
        )
        .bind(&pattern)
        .fetch_all(pool)
        .await
        .map_err(|e| CrvError::Database(format!("depot query failed: {e}")))?
    } else {
        sqlx::query(
            "SELECT DISTINCT ON (depot_path) depot_path, revision, action, file_type, digest, size
             FROM file_revisions
             ORDER BY depot_path, revision DESC",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| CrvError::Database(format!("depot query failed: {e}")))?
    };

    // Fetch client's have list
    let have_rows = sqlx::query(
        "SELECT depot_path, revision, digest FROM have WHERE client_id = $1",
    )
    .bind(client_id)
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("have query failed: {e}")))?;

    // Build have map: depot_path → (revision, digest)
    let have_map: std::collections::HashMap<String, (i32, String)> = have_rows
        .iter()
        .map(|r| {
            let path: String = r.get("depot_path");
            let rev: i32 = r.get("revision");
            let dig: String = r.get("digest");
            (path, (rev, dig))
        })
        .collect();

    let mut result: Vec<SyncFileEntry> = Vec::new();

    for df in &depot_files {
        let depot_path: String = df.get("depot_path");
        let head_rev: i32 = df.get("revision");
        let head_action: String = df.get("action");
        let head_digest: String = df.get("digest");
        let head_size: i64 = df.get("size");

        // Check if client needs this file
        let needs_content = if force {
            true
        } else if let Some((have_rev, have_digest)) = have_map.get(&depot_path) {
            if *have_rev >= head_rev {
                // Client has same or newer revision → skip
                continue;
            }
            // Digest mismatch → need update
            have_digest != &head_digest
        } else {
            // Not in have list → need full file
            true
        };

        // Determine action type for the sync entry
        let action = if needs_content && !have_map.contains_key(&depot_path) {
            FileActionType::Add
        } else {
            match head_action.as_str() {
                "add" => FileActionType::Add,
                "edit" => FileActionType::Edit,
                "delete" => FileActionType::Delete,
                "branch" => FileActionType::Branch,
                "integrate" => FileActionType::Integrate,
                _ => FileActionType::Edit,
            }
        };

        let file_type = match detect_type(&depot_path) {
            "text" => FileType::Text,
            _ => FileType::Binary,
        };

        result.push(SyncFileEntry {
            depot_path,
            revision: head_rev,
            action,
            file_type,
            file_size: head_size,
            digest: head_digest,
            needs_content,
        });
    }

    Ok(result)
}

/// Store sync results in the client's have list (called after successful file transfer).
pub async fn record_sync(
    pool: &PgPool,
    client_id: Uuid,
    entries: &[SyncFileEntry],
) -> Result<()> {
    for entry in entries {
        if entry.needs_content {
            sqlx::query(
                "INSERT INTO have (client_id, depot_path, revision, digest, file_size)
                 VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (client_id, depot_path)
                 DO UPDATE SET revision = $3, digest = $4, file_size = $5, sync_time = now()",
            )
            .bind(client_id)
            .bind(&entry.depot_path)
            .bind(entry.revision)
            .bind(&entry.digest)
            .bind(entry.file_size)
            .execute(pool)
            .await
            .map_err(|e| CrvError::Database(format!("have insert failed: {e}")))?;
        }
    }
    Ok(())
}

fn detect_type(path: &str) -> &str {
    let lower = path.to_lowercase();
    if lower.ends_with(".rs") || lower.ends_with(".py") || lower.ends_with(".js")
        || lower.ends_with(".ts") || lower.ends_with(".go") || lower.ends_with(".java")
        || lower.ends_with(".c") || lower.ends_with(".cpp") || lower.ends_with(".h")
        || lower.ends_with(".txt") || lower.ends_with(".md") || lower.ends_with(".toml")
        || lower.ends_with(".yaml") || lower.ends_with(".yml") || lower.ends_with(".json")
        || lower.ends_with(".xml") || lower.ends_with(".html") || lower.ends_with(".css")
        || lower.ends_with(".sh") || lower.ends_with(".bat")
    {
        "text"
    } else {
        "binary"
    }
}
