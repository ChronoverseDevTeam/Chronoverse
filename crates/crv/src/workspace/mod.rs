/// Local workspace state management (db.have equivalent).
/// Uses SQLite for persistent local tracking of synced file revisions.

use crv_shared::error::{CrvError, Result};
use crv_shared::types::HaveEntry;
use rusqlite::Connection;
use std::path::PathBuf;

/// Manages the local SQLite database that tracks which file revisions
/// are synced to this workspace.
pub struct LocalWorkspace {
    db: Connection,
    pub root: PathBuf,
}

impl LocalWorkspace {
    /// Open or create the local workspace database at the given root.
    pub fn open(root: PathBuf) -> Result<Self> {
        let db_path = root.join(".crv").join("db.have");
        std::fs::create_dir_all(db_path.parent().unwrap())
            .map_err(|e| CrvError::Storage(format!("cannot create .crv dir: {e}")))?;

        let db = Connection::open(&db_path)
            .map_err(|e| CrvError::Storage(format!("cannot open local db: {e}")))?;

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS have (
                depot_path TEXT NOT NULL,
                revision   INTEGER NOT NULL,
                digest     TEXT NOT NULL,
                file_size  INTEGER NOT NULL DEFAULT 0,
                sync_time  TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (depot_path)
            );
            CREATE TABLE IF NOT EXISTS config (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )
        .map_err(|e| CrvError::Storage(format!("cannot init local db: {e}")))?;

        Ok(Self { db, root })
    }

    /// Record a synced file revision.
    pub fn record_sync(&self, entry: &HaveEntry) -> Result<()> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO have (depot_path, revision, digest, file_size, sync_time)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
                rusqlite::params![entry.depot_path, entry.revision, entry.digest, entry.file_size],
            )
            .map_err(|e| CrvError::Storage(format!("record sync failed: {e}")))?;
        Ok(())
    }

    /// Get the synced revision for a depot path, if any.
    pub fn get_have(&self, depot_path: &str) -> Result<Option<HaveEntry>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT depot_path, revision, digest, file_size, sync_time
                 FROM have WHERE depot_path = ?1",
            )
            .map_err(|e| CrvError::Storage(format!("query have failed: {e}")))?;

        let mut rows = stmt
            .query_map(rusqlite::params![depot_path], |row| {
                Ok(HaveEntry {
                    client_id: uuid::Uuid::nil(),
                    depot_path: row.get(0)?,
                    revision: row.get(1)?,
                    digest: row.get(2)?,
                    file_size: row.get(3)?,
                    sync_time: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .unwrap_or_default()
                        .with_timezone(&chrono::Utc),
                })
            })
            .map_err(|e| CrvError::Storage(format!("query have failed: {e}")))?;

        Ok(rows.next().transpose().map_err(|e| {
            CrvError::Storage(format!("query have failed: {e}"))
        })?)
    }

    /// Remove a have entry (when a file is deleted from workspace).
    pub fn remove_have(&self, depot_path: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM have WHERE depot_path = ?1", rusqlite::params![depot_path])
            .map_err(|e| CrvError::Storage(format!("remove have failed: {e}")))?;
        Ok(())
    }

    /// Count synced files in the have list.
    pub fn count_have(&self) -> i64 {
        self.db.query_row("SELECT COUNT(*) FROM have", [], |r| r.get(0)).unwrap_or(0)
    }
}
