//! In-memory registry mapping chunk hashes to pending submit IDs.
//!
//! Populated by `pre_submit`, queried by the blob-event listener to know
//! which submit to extend, and cleaned up on commit / cancel / expire.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;

use iroh_blobs::Hash;

/// Tracks the association between chunk hashes and pending submits, plus
/// per-submit throttle state for expiry extensions.
#[derive(Debug)]
pub struct SubmitRegistry {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// chunk hash → set of submit IDs that declared this chunk.
    hash_to_submits: HashMap<Hash, HashSet<i64>>,
    /// submit_id → monotonic instant of the last successful expiry extension.
    last_extended: HashMap<i64, Instant>,
}

impl SubmitRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    /// Register all chunk hashes for a newly created pending submit.
    pub fn register(&self, submit_id: i64, hashes: impl IntoIterator<Item = Hash>) {
        let mut inner = self.inner.lock().unwrap();
        for h in hashes {
            inner.hash_to_submits.entry(h).or_default().insert(submit_id);
        }
        // Allow immediate first extension.
        inner.last_extended.remove(&submit_id);
    }

    /// Remove a submit from the registry (on commit, cancel, or expire).
    pub fn unregister(&self, submit_id: i64) {
        let mut inner = self.inner.lock().unwrap();
        inner.hash_to_submits.retain(|_, ids| {
            ids.remove(&submit_id);
            !ids.is_empty()
        });
        inner.last_extended.remove(&submit_id);
    }

    /// Look up which submit IDs are associated with a given blob hash.
    /// Returns an empty set if the hash is not registered.
    pub fn lookup(&self, hash: &Hash) -> HashSet<i64> {
        let inner = self.inner.lock().unwrap();
        inner
            .hash_to_submits
            .get(hash)
            .cloned()
            .unwrap_or_default()
    }

    /// Check whether enough time has passed since the last extension for
    /// `submit_id`.  If yes, update the timestamp and return `true`.
    /// If not, return `false` (caller should skip the DB write).
    pub fn try_mark_extended(&self, submit_id: i64, throttle: std::time::Duration) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();
        if let Some(last) = inner.last_extended.get(&submit_id) {
            if now.duration_since(*last) < throttle {
                return false;
            }
        }
        inner.last_extended.insert(submit_id, now);
        true
    }
}
