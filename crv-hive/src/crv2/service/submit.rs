use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::crv2::iroh::controller::pre_submit_controller::PreSubmitFile;
use crate::crv2::postgres::dao::{self, DaoError};
use crate::crv2::postgres::executor::{PostgreExecutor, PostgreExecutorError};

/// Default lock duration: 10 seconds in milliseconds.
/// Kept short so that locks are released quickly if the client disconnects.
/// Active blob uploads extend the expiry via iroh-blobs push events.
const DEFAULT_LOCK_DURATION_MS: i64 = 10 * 1000;

/// Public accessor so other modules (e.g. the event listener) can reuse
/// the same heartbeat interval.
pub const fn lock_duration_ms() -> i64 {
    DEFAULT_LOCK_DURATION_MS
}

// ── Result types ─────────────────────────────────────────────────────────────

pub struct PreSubmitResult {
    pub submit_id: i64,
    pub expires_at: i64,
}

pub struct SubmitResult {
    pub changelist_id: i64,
}

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SubmitServiceError {
    #[error("no files provided")]
    EmptyFiles,

    #[error("file path must not be empty")]
    EmptyPath,

    #[error("invalid action '{0}', expected add|edit|delete")]
    InvalidAction(String),

    #[error("files are locked by another submit: {0:?}")]
    FilesLocked(Vec<String>),

    #[error("submit {0} not found")]
    NotFound(i64),

    #[error("submit {0} is not pending (status: {1})")]
    NotPending(i64, String),

    #[error("file has no revision history (cannot edit/delete): {0}")]
    NoRevisionHistory(String),

    #[error("chunk not found in CAS: {0}")]
    MissingChunk(String),

    #[error("database error: {0}")]
    Dao(#[from] DaoError),

    #[error("executor error: {0}")]
    Executor(#[from] PostgreExecutorError),
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Pre-submit: create pending submit + lock files ───────────────────────────

pub async fn pre_submit(
    pg: &PostgreExecutor,
    author: &str,
    description: &str,
    files: &[PreSubmitFile],
) -> Result<PreSubmitResult, SubmitServiceError> {
    if files.is_empty() {
        return Err(SubmitServiceError::EmptyFiles);
    }

    // Validate inputs.
    for f in files {
        if f.path.trim().is_empty() {
            return Err(SubmitServiceError::EmptyPath);
        }
        match f.action.as_str() {
            "add" | "edit" | "delete" => {}
            other => return Err(SubmitServiceError::InvalidAction(other.to_string())),
        }
    }

    let now = now_ms();
    let expires_at = now + DEFAULT_LOCK_DURATION_MS;

    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

    // Run inside a transaction so that lock-check + insert is atomic.
    let result = pg
        .transaction::<PreSubmitResult, SubmitServiceError, _>(|txn| {
            // We need owned copies inside the closure.
            let author = author.to_string();
            let description = description.to_string();
            let paths = paths.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            let new_files: Vec<dao::submit::NewSubmitFile> = files
                .iter()
                .map(|f| dao::submit::NewSubmitFile {
                    submit_id: 0, // placeholder, filled after insert
                    path: f.path.clone(),
                    action: f.action.clone(),
                    chunk_hashes: f.chunk_hashes.clone(),
                    size: f.size,
                })
                .collect();

            Box::pin(async move {
                // 1. Check for lock conflicts.
                let path_refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
                let locked = dao::submit::find_locked_paths(txn, &path_refs, None).await?;
                if !locked.is_empty() {
                    return Err(SubmitServiceError::FilesLocked(locked));
                }

                // 2. Create the submit record.
                let submit_id = dao::submit::create(
                    txn,
                    dao::submit::NewSubmit {
                        author,
                        description,
                        created_at: now,
                        expires_at,
                    },
                )
                .await?;

                // 3. Insert submit files (with real submit_id).
                let submit_files: Vec<dao::submit::NewSubmitFile> = new_files
                    .into_iter()
                    .map(|mut f| {
                        f.submit_id = submit_id;
                        f
                    })
                    .collect();
                dao::submit::add_files(txn, submit_files).await?;

                Ok(PreSubmitResult { submit_id, expires_at })
            })
        })
        .await?;

    Ok(result)
}

// ── Submit: finalise a pending submit ────────────────────────────────────────

pub async fn submit(
    pg: &PostgreExecutor,
    cas_store: &crv_core::cas::CasStore,
    submit_id: i64,
) -> Result<SubmitResult, SubmitServiceError> {
    // 1. Load submit and verify it is pending.
    let submit_model = dao::submit::find_by_id(pg.connection(), submit_id)
        .await?
        .ok_or(SubmitServiceError::NotFound(submit_id))?;

    if !submit_model.is_pending() {
        return Err(SubmitServiceError::NotPending(
            submit_id,
            submit_model.status.clone(),
        ));
    }

    // 2. Load all submit files.
    let submit_files = dao::submit::find_files(pg.connection(), submit_id).await?;

    // 3. Verify all chunks exist in CAS.
    for sf in &submit_files {
        for hash_hex in sf.chunk_hash_list() {
            let parsed = blake3::Hash::from_hex(&hash_hex)
                .map_err(|_| SubmitServiceError::MissingChunk(hash_hex.clone()))?;
            let blob_id = crv_core::cas::BlobId::from_bytes(*parsed.as_bytes());
            if !cas_store.exists(blob_id).await.unwrap_or(false) {
                return Err(SubmitServiceError::MissingChunk(hash_hex));
            }
        }
    }

    let now = now_ms();

    // 4. Create changelist + file revisions in a single transaction.
    let changelist_id = pg
        .transaction::<i64, SubmitServiceError, _>(|txn| {
            let submit_files = submit_files.clone();
            Box::pin(async move {
                // 4a. Insert changelist.
                let cl_id = dao::changelist::insert(
                    txn,
                    dao::changelist::NewChangelist {
                        author: submit_model.author.clone(),
                        description: submit_model.description.clone(),
                        committed_at: now,
                    },
                )
                .await?;

                // 4b. For each file, compute generation/revision and insert file_revision.
                let mut revisions = Vec::with_capacity(submit_files.len());
                for sf in &submit_files {
                    let action = sf.action.as_str();
                    let latest = dao::file_revision::find_latest_by_path(txn, &sf.path).await?;

                    let (generation, revision, is_deletion) = match action {
                        "add" => {
                            // Ensure file record exists.
                            dao::file::upsert(txn, &sf.path, now).await?;

                            match &latest {
                                // Brand new file.
                                None => (1_i64, 1_i64, false),
                                // File was previously deleted — start new generation.
                                Some(prev) if prev.is_deletion => {
                                    (prev.generation + 1, 1, false)
                                }
                                // File already exists and isn't deleted — this is
                                // actually an edit, but we accept it as add.
                                Some(prev) => (prev.generation, prev.revision + 1, false),
                            }
                        }
                        "edit" => {
                            let prev = latest.ok_or_else(|| {
                                SubmitServiceError::NoRevisionHistory(sf.path.clone())
                            })?;
                            if prev.is_deletion {
                                return Err(SubmitServiceError::NoRevisionHistory(
                                    sf.path.clone(),
                                ));
                            }
                            (prev.generation, prev.revision + 1, false)
                        }
                        "delete" => {
                            let prev = latest.ok_or_else(|| {
                                SubmitServiceError::NoRevisionHistory(sf.path.clone())
                            })?;
                            if prev.is_deletion {
                                return Err(SubmitServiceError::NoRevisionHistory(
                                    sf.path.clone(),
                                ));
                            }
                            (prev.generation, prev.revision + 1, true)
                        }
                        _ => unreachable!(), // validated in pre_submit
                    };

                    revisions.push(dao::file_revision::NewFileRevision {
                        path: sf.path.clone(),
                        generation,
                        revision,
                        changelist_id: cl_id,
                        chunk_hashes: if is_deletion {
                            vec![]
                        } else {
                            sf.chunk_hash_list()
                        },
                        size: if is_deletion { 0 } else { sf.size },
                        is_deletion,
                        created_at: now,
                    });
                }

                dao::file_revision::insert_many(txn, revisions).await?;

                // 4c. Transition submit to committed.
                dao::submit::mark_committed(txn, submit_id, cl_id).await?;

                Ok(cl_id)
            })
        })
        .await?;

    Ok(SubmitResult { changelist_id })
}

// ── Cancel submit ────────────────────────────────────────────────────────────

pub async fn cancel_submit(
    pg: &PostgreExecutor,
    submit_id: i64,
) -> Result<(), SubmitServiceError> {
    let submit_model = dao::submit::find_by_id(pg.connection(), submit_id)
        .await?
        .ok_or(SubmitServiceError::NotFound(submit_id))?;

    if !submit_model.is_pending() {
        return Err(SubmitServiceError::NotPending(
            submit_id,
            submit_model.status.clone(),
        ));
    }

    dao::submit::mark_cancelled(pg.connection(), submit_id).await?;
    Ok(())
}
