use crv_shared::error::{CrvError, Result};
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

use crate::storage::depot::Depot;

/// Result of an integration operation.
#[derive(Debug)]
pub struct IntegrateResult {
    pub files_branched: i32,
    pub integration_records: i32,
}

/// Branch files from a source path to a target path.
///
/// For each file matching the source pattern at its head revision:
/// 1. Copy the blob (reuse same digest)
/// 2. Create a new file_revisions entry at the target path with action='branch'
/// 3. Record the integration in the integrations table
pub async fn integrate_files(
    pool: &PgPool,
    _depot: &Depot,
    source_spec: &str,
    target_spec: &str,
    user_id: Uuid,
    _change_id: Option<Uuid>,
    action: &str, // "branch_from", "merge_from", "copy_from"
) -> Result<IntegrateResult> {
    // Resolve source files: get head revision of each matching file
    let source_pattern = source_spec.replace("...", "%");
    let source_rows = sqlx::query(
        "SELECT DISTINCT ON (fr.depot_path) fr.depot_path, fr.revision, fr.digest, fr.file_type, fr.action, fr.size
         FROM file_revisions fr
         WHERE fr.depot_path LIKE $1
         ORDER BY fr.depot_path, fr.revision DESC",
    )
    .bind(&source_pattern)
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("source query failed: {e}")))?;

    if source_rows.is_empty() {
        return Err(CrvError::NotFound(format!(
            "no files match source pattern '{source_spec}'"
        )));
    }

    // Determine target path prefix: replace source prefix with target prefix
    let (src_prefix, tgt_prefix) = parse_prefixes(source_spec, target_spec)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| CrvError::Database(format!("tx begin: {e}")))?;

    let mut files_branched = 0;
    let mut integ_records = 0;

    for row in &source_rows {
        let src_path: String = row.get("depot_path");
        let src_rev: i32 = row.get("revision");
        let digest: String = row.get("digest");
        let file_type: String = row.get("file_type");
        let _src_action: String = row.get("action");
        let size: i64 = row.get("size");

        // Compute target path
        let tgt_path = if let (Some(sp), Some(tp)) = (&src_prefix, &tgt_prefix) {
            src_path.replacen(sp, tp, 1)
        } else {
            format!("{}/{}", target_spec.trim_end_matches('/'), src_path.rsplit('/').next().unwrap_or(""))
        };

        // Determine next revision for target
        let tgt_max: Option<i32> = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT MAX(revision) FROM file_revisions WHERE depot_path = $1",
        )
        .bind(&tgt_path)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("target rev lookup: {e}")))?;

        let tgt_rev = tgt_max.map_or(1, |r| r + 1);

        // Determine integration action types.
        // file_action: must match file_revisions CHECK (add/edit/delete/branch/integrate/move_add/move_delete)
        // integ_record_action: must match integrations CHECK (branch_from/merge_from/copy_from/delete_from/ignore)
        let (file_action, integ_record_action) = match action {
            "merge" | "merge_from" => {
                if tgt_max.is_some() { ("integrate", "merge_from") } else { ("branch", "branch_from") }
            }
            "copy" | "copy_from" => ("branch", "copy_from"),
            _ => ("branch", "branch_from"), // "branch" or anything else
        };

        // Insert revision record for target (no changelist for integrations)
        let change_id: Option<Uuid> = None;
        sqlx::query(
            "INSERT INTO file_revisions (depot_path, revision, change_id, action, file_type, digest, size)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&tgt_path)
        .bind(tgt_rev)
        .bind(change_id)
        .bind(file_action)
        .bind(&file_type)
        .bind(&digest)
        .bind(size)
        .execute(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("target rev insert: {e}")))?;

        // Record integration
        sqlx::query(
            "INSERT INTO integrations (source_path, source_start_rev, source_end_rev, target_path, target_start_rev, target_end_rev, action, change_id, user_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&src_path)
        .bind(1)
        .bind(src_rev)
        .bind(&tgt_path)
        .bind(tgt_rev)
        .bind(tgt_rev)
        .bind(integ_record_action)
        .bind(change_id) // None — no changelist
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| CrvError::Database(format!("integration record: {e}")))?;

        files_branched += 1;
        integ_records += 1;
    }

    tx.commit()
        .await
        .map_err(|e| CrvError::Database(format!("tx commit: {e}")))?;

    tracing::info!(
        "Integration complete: {} files {} from '{}' to '{}'",
        files_branched, action, source_spec, target_spec
    );

    Ok(IntegrateResult {
        files_branched,
        integration_records: integ_records,
    })
}

/// Parse source and target file specs to extract prefixes for path mapping.
fn parse_prefixes(
    source: &str,
    target: &str,
) -> Result<(Option<String>, Option<String>)> {
    let src = source.trim_end_matches("...").trim_end_matches('/');
    let tgt = target.trim_end_matches("...").trim_end_matches('/');

    if src.contains("...") || tgt.contains("...") {
        // With wildcards, we can't do simple prefix replacement
        return Ok((None, None));
    }

    Ok((Some(src.to_string()), Some(tgt.to_string())))
}

/// List integration history for a file or path pattern.
pub async fn list_integrations(
    pool: &PgPool,
    depot_path: Option<&str>,
) -> Result<Vec<IntegrationInfo>> {
    let rows = if let Some(path) = depot_path {
        let pattern = path.replace("...", "%");
        sqlx::query(
            "SELECT i.source_path, i.source_start_rev, i.source_end_rev,
                    i.target_path, i.target_start_rev, i.target_end_rev,
                    i.action, i.created_at, u.name as user_name
             FROM integrations i JOIN users u ON i.user_id = u.id
             WHERE i.source_path LIKE $1 OR i.target_path LIKE $1
             ORDER BY i.created_at DESC
             LIMIT 200",
        )
        .bind(&pattern)
        .fetch_all(pool)
        .await
        .map_err(|e| CrvError::Database(format!("integration list: {e}")))?
    } else {
        sqlx::query(
            "SELECT i.source_path, i.source_start_rev, i.source_end_rev,
                    i.target_path, i.target_start_rev, i.target_end_rev,
                    i.action, i.created_at, u.name as user_name
             FROM integrations i JOIN users u ON i.user_id = u.id
             ORDER BY i.created_at DESC
             LIMIT 200",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| CrvError::Database(format!("integration list: {e}")))?
    };

    Ok(rows
        .iter()
        .map(|r| IntegrationInfo {
            source_path: r.get("source_path"),
            source_start_rev: r.get("source_start_rev"),
            source_end_rev: r.get("source_end_rev"),
            target_path: r.get("target_path"),
            target_start_rev: r.get("target_start_rev"),
            target_end_rev: r.get("target_end_rev"),
            action: r.get("action"),
            user_name: r.get("user_name"),
            created_at: r.get("created_at"),
        })
        .collect())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IntegrationInfo {
    pub source_path: String,
    pub source_start_rev: i32,
    pub source_end_rev: i32,
    pub target_path: String,
    pub target_start_rev: i32,
    pub target_end_rev: i32,
    pub action: String,
    pub user_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
