use crv_shared::error::{CrvError, Result};
use sqlx::PgPool;
use sqlx::Row;
use uuid::Uuid;

/// Snapshot the current head revisions of matching files into a label.
///
/// For each file matching the filespec at its head revision:
/// - UPSERT into label_revisions (label_id, depot_path, revision)
pub async fn sync_label(
    pool: &PgPool,
    label_id: Uuid,
    filespec: &str,
) -> Result<i32> {
    let pattern = filespec.replace("...", "%");

    let rows = sqlx::query(
        "SELECT DISTINCT ON (depot_path) depot_path, revision
         FROM file_revisions
         WHERE depot_path LIKE $1
         ORDER BY depot_path, revision DESC",
    )
    .bind(&pattern)
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("label sync query: {e}")))?;

    if rows.is_empty() {
        return Err(CrvError::NotFound(format!(
            "no files match '{filespec}'"
        )));
    }

    let mut count = 0;
    for row in &rows {
        let depot_path: String = row.get("depot_path");
        let revision: i32 = row.get("revision");

        sqlx::query(
            "INSERT INTO label_revisions (label_id, depot_path, revision)
             VALUES ($1, $2, $3)
             ON CONFLICT (label_id, depot_path) DO UPDATE SET revision = $3",
        )
        .bind(label_id)
        .bind(&depot_path)
        .bind(revision)
        .execute(pool)
        .await
        .map_err(|e| CrvError::Database(format!("label sync insert: {e}")))?;

        count += 1;
    }

    tracing::info!("Label sync: {} files tagged", count);
    Ok(count)
}

/// Get the list of revisions tagged by a label.
pub async fn list_label_revisions(
    pool: &PgPool,
    label_id: Uuid,
) -> Result<Vec<LabelRevisionInfo>> {
    let rows = sqlx::query(
        "SELECT lr.depot_path, lr.revision, fr.digest, fr.size
         FROM label_revisions lr
         JOIN file_revisions fr ON lr.depot_path = fr.depot_path AND lr.revision = fr.revision
         WHERE lr.label_id = $1
         ORDER BY lr.depot_path",
    )
    .bind(label_id)
    .fetch_all(pool)
    .await
    .map_err(|e| CrvError::Database(format!("label revisions query: {e}")))?;

    Ok(rows
        .iter()
        .map(|r| LabelRevisionInfo {
            depot_path: r.get("depot_path"),
            revision: r.get("revision"),
            digest: r.get("digest"),
            size: r.get("size"),
        })
        .collect())
}

/// Remove all revisions from a label (but keep the label).
pub async fn clear_label(pool: &PgPool, label_id: Uuid) -> Result<i32> {
    let result = sqlx::query("DELETE FROM label_revisions WHERE label_id = $1")
        .bind(label_id)
        .execute(pool)
        .await
        .map_err(|e| CrvError::Database(format!("label clear: {e}")))?;

    Ok(result.rows_affected() as i32)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LabelRevisionInfo {
    pub depot_path: String,
    pub revision: i32,
    pub digest: String,
    pub size: i64,
}
