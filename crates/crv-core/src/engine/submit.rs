use crv_shared::error::{CrvError, Result};
use crv_shared::types::FileActionType;
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

use crate::storage::depot::Depot;

/// Result of a successful submit operation.
#[derive(Debug)]
pub struct SubmitResult {
    pub change_number: i64,
    pub files_submitted: i32,
}

/// Submit a changelist: atomically commit all opened files to the depot.
///
/// This function:
/// 1. Validates the changelist exists and is pending
/// 2. Reads the client's opened files
/// 3. For each file, computes the next revision, stores the blob, creates a rev record
/// 4. Updates the have list for the submitting client
/// 5. Marks the changelist as submitted
/// 6. Clears working entries
pub async fn submit_changelist(
    pool: &PgPool,
    depot: &Depot,
    change_id: Uuid,
    client_id: Uuid,
    user_id: Uuid,
) -> Result<SubmitResult> {
    // ── Validate changelist ────────────────────────────────────────
    let change = sqlx::query(
        "SELECT id, status, number FROM changes WHERE id = $1 AND client_id = $2",
    )
    .bind(change_id)
    .bind(client_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| CrvError::Database(format!("change lookup failed: {e}")))?
    .ok_or_else(|| CrvError::NotFound("changelist not found".into()))?;

    let status: String = change.get("status");
    if status != "pending" {
        return Err(CrvError::InvalidInput(format!(
            "changelist is already {status}"
        )));
    }

    // ── Get opened files ───────────────────────────────────────────
    let opened_rows = sqlx::query(
        "SELECT w.depot_path, w.action, w.base_revision
         FROM working w
         WHERE w.client_id = $1 AND (w.change_id = $2 OR w.change_id IS NULL)
         ORDER BY w.depot_path",
    )
    .bind(client_id)
    .bind(change_id)
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("working lookup failed: {e}")))?;

    if opened_rows.is_empty() {
        return Err(CrvError::InvalidInput("no files to submit".into()));
    }

    // ── Check locks ────────────────────────────────────────────────
    let paths: Vec<String> = opened_rows.iter().map(|r| r.get("depot_path")).collect();
    crate::engine::lock::check_locks(pool, &paths, user_id).await?;

    // ── Start transaction ──────────────────────────────────────────
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| CrvError::Database(format!("tx begin failed: {e}")))?;

    // Use existing BIGSERIAL number (already unique)
    let change_number: i64 = change.get("number");

    let mut files_submitted: i32 = 0;

    for row in &opened_rows {
        let depot_path: String = row.get("depot_path");
        let action: String = row.get("action");
        let _base_revision: Option<i32> = row.get("base_revision");

        let action_type = match action.as_str() {
            "add" => FileActionType::Add,
            "edit" => FileActionType::Edit,
            "delete" => FileActionType::Delete,
            "branch" => FileActionType::Branch,
            "integrate" => FileActionType::Integrate,
            _ => {
                return Err(CrvError::InvalidInput(format!(
                    "unknown action '{action}' for '{depot_path}'"
                )));
            }
        };

        // Compute next revision
        let current_max: Option<i32> = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT MAX(revision) FROM file_revisions WHERE depot_path = $1",
        )
        .bind(&depot_path)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("rev lookup failed: {e}")))?;

        let new_rev = current_max.map_or(1, |r| r + 1);

        // Determine digest and size
        let (digest, file_size, file_type) = match action_type {
            FileActionType::Delete => {
                ("0000000000000000000000000000000000000000000000000000000000000000".to_string(), 0i64, "text".to_string())
            }
            _ => {
                // Look for uploaded content: check staging file written by upload handler.
                // Normalize depot_path: strip all leading slashes to match upload handler.
                let normalized = depot_path.trim_start_matches('/');
                let staging_key = format!("staging_{}", normalized.replace('/', "_"));
                let staging_path = depot.blob_path(&staging_key);
                
                let content = if staging_path.exists() {
                    std::fs::read(&staging_path)
                        .map_err(|e| CrvError::Storage(format!("staging read failed: {e}")))?
                } else {
                    return Err(CrvError::InvalidInput(format!(
                        "no content uploaded for '{depot_path}' — upload file content via POST /files/content/{depot_path} before submit"
                    )));
                };

                let d = Depot::compute_digest(&content);
                let s = content.len() as i64;

                // Store in depot
                depot.store_blob(&content).await?;

                // Clean up staging
                let _ = std::fs::remove_file(&staging_path);

                // Determine file type
                let ft = detect_file_type(&depot_path, &content);

                (d, s, ft)
            }
        };

        // Insert revision record
        sqlx::query(
            "INSERT INTO file_revisions (depot_path, revision, change_id, action, file_type, digest, size)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&depot_path)
        .bind(new_rev)
        .bind(change_id)
        .bind(action)
        .bind(&file_type)
        .bind(&digest)
        .bind(file_size)
        .execute(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("rev insert failed: {e}")))?;

        // Update have list for submitting client
        sqlx::query(
            "INSERT INTO have (client_id, depot_path, revision, digest, file_size)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (client_id, depot_path)
             DO UPDATE SET revision = $3, digest = $4, file_size = $5, sync_time = now()",
        )
        .bind(client_id)
        .bind(&depot_path)
        .bind(new_rev)
        .bind(&digest)
        .bind(file_size)
        .execute(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("have update failed: {e}")))?;

        files_submitted += 1;
    }

    // Mark changelist as submitted
    sqlx::query("UPDATE changes SET status = 'submitted', number = $1 WHERE id = $2")
        .bind(change_number)
        .bind(change_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("change update failed: {e}")))?;

    // Clear working entries for this changelist
    sqlx::query(
        "DELETE FROM working WHERE client_id = $1 AND (change_id = $2 OR change_id IS NULL)",
    )
    .bind(client_id)
    .bind(change_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| CrvError::Database(format!("working cleanup failed: {e}")))?;

    // Commit transaction
    tx.commit()
        .await
        .map_err(|e| CrvError::Database(format!("tx commit failed: {e}")))?;

    // Release locks for submitted files
    for path in &paths {
        let _ = crate::engine::lock::unlock_file(pool, path, user_id).await;
    }

    tracing::info!(
        "Submit complete: change {} by user {}, {} files",
        change_number, user_id, files_submitted
    );

    Ok(SubmitResult {
        change_number,
        files_submitted,
    })
}

/// Simple heuristic to detect file type.
fn detect_file_type(path: &str, _content: &[u8]) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".rs") || lower.ends_with(".py") || lower.ends_with(".js")
        || lower.ends_with(".ts") || lower.ends_with(".go") || lower.ends_with(".java")
        || lower.ends_with(".c") || lower.ends_with(".cpp") || lower.ends_with(".h")
        || lower.ends_with(".txt") || lower.ends_with(".md") || lower.ends_with(".toml")
        || lower.ends_with(".yaml") || lower.ends_with(".yml") || lower.ends_with(".json")
        || lower.ends_with(".xml") || lower.ends_with(".html") || lower.ends_with(".css")
    {
        "text".into()
    } else {
        "binary".into()
    }
}
