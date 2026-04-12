//! Database schema initialisation for the crv2 Postgres layer.
//!
//! All DDL statements are idempotent (`IF NOT EXISTS` guards), so [`init`]
//! is safe to call on an already-initialised database without causing data
//! loss or errors.
//!
//! A Postgres session-level advisory lock is held for the duration of the
//! DDL so that multiple service instances starting up simultaneously
//! serialise on schema creation rather than racing on concurrent DDL.

use sea_orm::{ConnectionTrait, DbErr, Statement};
use thiserror::Error;

/// Errors that can occur while initialising the crv2 schema.
#[derive(Debug, Error)]
pub enum InitError {
    #[error("database error during crv2 schema init: {0}")]
    Db(#[from] DbErr),
}

/// Initialise the crv2 schema on `db`.
///
/// Creates the following tables if they do not already exist (in dependency
/// order):
///
/// 1. `users`
/// 2. `files`
/// 3. `changelists`
/// 4. `file_revisions` (FK → `files`, FK → `changelists`)
///
/// Supporting indexes are also created idempotently.
pub async fn init<C>(db: &C) -> Result<(), InitError>
where
    C: ConnectionTrait,
{
    let backend = db.get_database_backend();

    // Serialise concurrent initialisations with an advisory lock.
    // Key 248031658 is reserved for crv2 schema init.
    db.execute(Statement::from_string(
        backend,
        "SELECT pg_advisory_lock(248031658)".to_string(),
    ))
    .await?;

    let result = run_ddl(db).await;

    // Always release the lock, even if DDL failed.
    let _ = db
        .execute(Statement::from_string(
            backend,
            "SELECT pg_advisory_unlock(248031658)".to_string(),
        ))
        .await;

    result
}

async fn run_ddl<C>(db: &C) -> Result<(), InitError>
where
    C: ConnectionTrait,
{
    let backend = db.get_database_backend();

    // ── users ────────────────────────────────────────────────────────────────
    db.execute(Statement::from_string(
        backend,
        r#"
        CREATE TABLE IF NOT EXISTS users (
            username      TEXT   NOT NULL PRIMARY KEY,
            password_hash TEXT   NOT NULL,
            created_at    BIGINT NOT NULL
        )
        "#
        .to_string(),
    ))
    .await?;

    // ── files ────────────────────────────────────────────────────────────────
    db.execute(Statement::from_string(
        backend,
        r#"
        CREATE TABLE IF NOT EXISTS files (
            path       TEXT   NOT NULL PRIMARY KEY,
            created_at BIGINT NOT NULL
        )
        "#
        .to_string(),
    ))
    .await?;

    // ── changelists ──────────────────────────────────────────────────────────
    db.execute(Statement::from_string(
        backend,
        r#"
        CREATE TABLE IF NOT EXISTS changelists (
            id           BIGSERIAL PRIMARY KEY,
            author       TEXT      NOT NULL,
            description  TEXT      NOT NULL,
            committed_at BIGINT    NOT NULL
        )
        "#
        .to_string(),
    ))
    .await?;

    // ── file_revisions ───────────────────────────────────────────────────────
    // Must be created after `files` and `changelists` due to FK constraints.
    db.execute(Statement::from_string(
        backend,
        r#"
        CREATE TABLE IF NOT EXISTS file_revisions (
            path          TEXT    NOT NULL,
            generation    BIGINT  NOT NULL,
            revision      BIGINT  NOT NULL,
            changelist_id BIGINT  NOT NULL,
            chunk_hashes  JSONB   NOT NULL,
            size          BIGINT  NOT NULL,
            is_deletion   BOOLEAN NOT NULL,
            created_at    BIGINT  NOT NULL,
            PRIMARY KEY (path, generation, revision),
            CONSTRAINT fk_file_revisions_path
                FOREIGN KEY (path)
                REFERENCES files (path)
                ON DELETE CASCADE
                ON UPDATE CASCADE,
            CONSTRAINT fk_file_revisions_changelist_id
                FOREIGN KEY (changelist_id)
                REFERENCES changelists (id)
                ON DELETE RESTRICT
                ON UPDATE CASCADE
        )
        "#
        .to_string(),
    ))
    .await?;

    // ── submits ──────────────────────────────────────────────────────────────
    db.execute(Statement::from_string(
        backend,
        r#"
        CREATE TABLE IF NOT EXISTS submits (
            id            BIGSERIAL PRIMARY KEY,
            author        TEXT      NOT NULL,
            description   TEXT      NOT NULL,
            status        TEXT      NOT NULL DEFAULT 'pending',
            changelist_id BIGINT,
            created_at    BIGINT    NOT NULL,
            expires_at    BIGINT    NOT NULL,
            CONSTRAINT fk_submits_changelist_id
                FOREIGN KEY (changelist_id)
                REFERENCES changelists (id)
                ON DELETE SET NULL
                ON UPDATE CASCADE
        )
        "#
        .to_string(),
    ))
    .await?;

    // ── submit_files ─────────────────────────────────────────────────────────
    // Must be created after `submits` and `files` due to FK constraints.
    db.execute(Statement::from_string(
        backend,
        r#"
        CREATE TABLE IF NOT EXISTS submit_files (
            submit_id    BIGINT NOT NULL,
            path         TEXT   NOT NULL,
            action       TEXT   NOT NULL,
            chunk_hashes JSONB  NOT NULL,
            size         BIGINT NOT NULL,
            PRIMARY KEY (submit_id, path),
            CONSTRAINT fk_submit_files_submit_id
                FOREIGN KEY (submit_id)
                REFERENCES submits (id)
                ON DELETE CASCADE
                ON UPDATE CASCADE,
            CONSTRAINT fk_submit_files_path
                FOREIGN KEY (path)
                REFERENCES files (path)
                ON DELETE CASCADE
                ON UPDATE CASCADE
        )
        "#
        .to_string(),
    ))
    .await?;

    // ── indexes ───────────────────────────────────────────────────────────────

    // Covers latest-revision look-ups and range scans over (path, generation).
    db.execute(Statement::from_string(
        backend,
        "CREATE INDEX IF NOT EXISTS idx_file_revisions_path \
         ON file_revisions (path, generation, revision)"
            .to_string(),
    ))
    .await?;

    // Covers fetching all file revisions that belong to a changelist.
    db.execute(Statement::from_string(
        backend,
        "CREATE INDEX IF NOT EXISTS idx_file_revisions_changelist_id \
         ON file_revisions (changelist_id)"
            .to_string(),
    ))
    .await?;

    // Fast lock-conflict check: find pending submits that reference a path.
    db.execute(Statement::from_string(
        backend,
        "CREATE INDEX IF NOT EXISTS idx_submit_files_path \
         ON submit_files (path)"
            .to_string(),
    ))
    .await?;

    // Find submits by status (e.g. expired pending submits).
    db.execute(Statement::from_string(
        backend,
        "CREATE INDEX IF NOT EXISTS idx_submits_status \
         ON submits (status)"
            .to_string(),
    ))
    .await?;

    Ok(())
}
