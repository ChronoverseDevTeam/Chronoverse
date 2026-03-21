//! Content-addressable binary blob store backed by [iroh-blobs].
//!
//! # Design
//!
//! Every blob is immutable and identified by its BLAKE3 hash ([`BlobId`]).
//! Identical content is deduplicated automatically.
//!
//! ## Garbage-collection & pinning
//!
//! Blobs without a live reference may be removed by the garbage collector.
//! There are two kinds of references:
//!
//! * **[`TempPin`]** – an in-process RAII guard returned by every write
//!   operation.  Dropping it without first creating a named pin will eventually
//!   cause the blob to be collected.
//! * **Named pins** – stored persistently (survive process restarts).
//!   Managed with [`CasStore::pin`] / [`CasStore::unpin`].
//!
//! Typical write workflow:
//! ```text
//! let pin = store.put_bytes(data).await?;         // TempPin keeps blob alive
//! store.pin(pin.hash(), b"myfile/v3").await?;     // promote to named pin
//! drop(pin);                                       // TempPin no longer needed
//! ```
//!
//! # Stores
//!
//! | Constructor | Persistence | Use case |
//! |---|---|---|
//! | [`CasStore::memory()`] | none | tests, caches |
//! | [`CasStore::memory_with_gc`] | none | long-running in-process |
//! | [`CasStore::persistent`] | redb + flat files | production |
//!
//! ## Bulk writes (海量小文件)
//!
//! For ingesting millions of small blobs call [`CasStore::batch`] to open a
//! [`CasBatch`] session.  All blobs added within a batch share a single actor
//! scope, which allows the store to coalesce redb transactions and reduces
//! per-blob overhead significantly.
//!
//! ```text
//! let mut batch = store.batch().await?;
//! for (path, data) in files {
//!     let id = batch.put_bytes(data).await?;
//!     store.pin(id, path.as_bytes()).await?;
//! }
//! drop(batch);   // scope closed; named pins keep everything alive
//! ```

mod error;
pub use error::{CasError, Result};

use std::{io, path::Path, time::Duration};

use bytes::Bytes;
use iroh_blobs::{
    Hash, HashAndFormat,
    api::{
        Store,
        TempTag,
        blobs::{Batch, BlobReader, BlobStatus},
        tags::TagInfo,
    },
    store::mem::{MemStore, Options as MemOptions},
    store::GcConfig,
};

// ─── Re-exported types ───────────────────────────────────────────────────────

/// Content-addressed blob identifier (BLAKE3 hash, 32 bytes).
///
/// Two blobs with the same content will always have the same `BlobId`.
pub type BlobId = Hash;

/// A transient GC guard for a freshly stored blob.
///
/// While this value is live, the associated blob is guaranteed not to be
/// garbage-collected.  Call [`CasStore::pin`] with a stable name to keep the
/// blob after dropping this guard.
pub type TempPin = TempTag;

/// Metadata for a single named pin (returned by [`CasStore::list_pins`]).
pub type PinInfo = TagInfo;

// ─── CasStore ────────────────────────────────────────────────────────────────

/// Content-addressable binary blob store.
///
/// Cheaply cloneable; all clones share the same underlying store actor.
#[derive(Clone)]
pub struct CasStore {
    inner: Store,
}

impl CasStore {
    // ── Constructors ─────────────────────────────────────────────────────────

    /// Create a non-persistent, in-memory store (no garbage collection).
    ///
    /// Suitable for tests and short-lived processes.
    pub fn memory() -> Self {
        Self {
            inner: MemStore::new().into(),
        }
    }

    /// Create a non-persistent, in-memory store with periodic garbage
    /// collection.
    ///
    /// Blobs that have no [`TempPin`] or named pin will be removed roughly
    /// every `gc_interval`.
    pub fn memory_with_gc(gc_interval: Duration) -> Self {
        let opts = MemOptions {
            gc_config: Some(GcConfig {
                interval: gc_interval,
                add_protected: Default::default(),
            }),
        };
        Self {
            inner: MemStore::new_with_opts(opts).into(),
        }
    }

    /// Open (or create) a file-system backed store at `data_dir`.
    ///
    /// The directory is created if it does not exist.  Pass an existing
    /// directory on subsequent calls to reopen the same store.
    pub async fn persistent(data_dir: impl AsRef<Path>) -> Result<Self> {
        use iroh_blobs::store::fs::FsStore;
        let store = FsStore::load(data_dir)
            .await
            .map_err(CasError::store)?;
        Ok(Self {
            inner: store.into(),
        })
    }

    // ── Write operations ─────────────────────────────────────────────────────

    /// Store raw bytes.
    ///
    /// Returns a [`TempPin`] containing the [`BlobId`] (`pin.hash()`).
    /// Hold onto it or call [`Self::pin`] before dropping.
    pub async fn put_bytes(&self, data: impl Into<Bytes>) -> Result<TempPin> {
        self.inner
            .blobs()
            .add_bytes(data.into())
            .temp_tag()
            .await
            .map_err(CasError::store)
    }

    /// Store data arriving from an async byte stream.
    ///
    /// The stream must yield `io::Result<Bytes>` chunks; use
    /// [`tokio_util::io::ReaderStream`] to wrap an `AsyncRead`.
    pub async fn put_stream<S>(&self, data: S) -> Result<TempPin>
    where
        S: futures_core::Stream<Item = io::Result<Bytes>> + Send + Sync + 'static,
    {
        self.inner
            .blobs()
            .add_stream(data)
            .await
            .temp_tag()
            .await
            .map_err(CasError::store)
    }

    /// Store a file by **copying** it into the store.
    ///
    /// The source file is left unmodified.
    pub async fn put_path(&self, path: impl AsRef<Path>) -> Result<TempPin> {
        self.inner
            .blobs()
            .add_path(path)
            .temp_tag()
            .await
            .map_err(CasError::store)
    }

    // ── Read operations ───────────────────────────────────────────────────────

    /// Get a lazy async reader for a stored blob.
    ///
    /// The reader implements [`tokio::io::AsyncRead`] and
    /// [`tokio::io::AsyncSeek`].  Attempting to read a missing or incomplete
    /// blob will yield an `io::Error`.
    ///
    /// This call is infallible and allocates no I/O until the first read.
    pub fn reader(&self, id: BlobId) -> BlobReader {
        self.inner.blobs().reader(id)
    }

    /// Read the complete bytes of a blob into memory.
    ///
    /// Returns `None` if the blob is not present or incomplete.
    pub async fn get_bytes(&self, id: BlobId) -> Result<Option<Bytes>> {
        use tokio::io::AsyncReadExt as _;

        match self
            .inner
            .blobs()
            .status(id)
            .await
            .map_err(CasError::store)?
        {
            BlobStatus::Complete { .. } => {}
            _ => return Ok(None),
        }

        let mut reader = self.inner.blobs().reader(id);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await?;
        Ok(Some(Bytes::from(buf)))
    }

    /// Return `true` if the blob is fully stored.
    pub async fn exists(&self, id: BlobId) -> Result<bool> {
        self.inner
            .blobs()
            .has(id)
            .await
            .map_err(CasError::store)
    }

    /// Return the detailed storage status of a blob.
    ///
    /// Useful for resumable transfers: a `Partial` blob can be completed later.
    pub async fn status(&self, id: BlobId) -> Result<BlobStatus> {
        self.inner
            .blobs()
            .status(id)
            .await
            .map_err(CasError::store)
    }

    // ── Export ────────────────────────────────────────────────────────────────

    /// Export a blob to a local path (copy semantics).
    ///
    /// Parent directories are created automatically.
    /// Returns [`CasError::NotFound`] if the blob is absent or incomplete.
    pub async fn export_to_path(&self, id: BlobId, dest: impl AsRef<Path>) -> Result<()> {
        if !self.exists(id).await? {
            return Err(CasError::NotFound(id));
        }

        let dest = dest.as_ref();
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let mut reader = self.inner.blobs().reader(id);
        let mut file = tokio::fs::File::create(dest).await?;
        tokio::io::copy(&mut reader, &mut file).await?;
        Ok(())
    }

    // ── Named pins (persistent GC roots) ───────────────────────────────────

    /// Create or update a named pin pointing to `id`.
    ///
    /// The blob will be protected from garbage collection and will survive
    /// process restarts for as long as this pin exists.
    ///
    /// `name` is an arbitrary byte key; use a structured scheme such as
    /// `b"depot://path/to/file@rev"` for easy prefix-scanning.
    pub async fn pin(&self, id: BlobId, name: impl AsRef<[u8]>) -> Result<()> {
        self.inner
            .tags()
            .set(name, HashAndFormat::raw(id))
            .await
            .map_err(CasError::store)
    }

    /// Remove a named pin.
    ///
    /// If no other pins or [`TempPin`]s reference the blob, it becomes
    /// eligible for garbage collection.
    ///
    /// Removing a pin that does not exist is a no-op.
    pub async fn unpin(&self, name: impl AsRef<[u8]>) -> Result<()> {
        self.inner
            .tags()
            .delete(name)
            .await
            .map_err(CasError::store)?;
        Ok(())
    }

    /// Look up which blob a named pin points to.
    ///
    /// Returns `None` if the pin does not exist.
    pub async fn get_pin(&self, name: impl AsRef<[u8]>) -> Result<Option<BlobId>> {
        let info = self
            .inner
            .tags()
            .get(name)
            .await
            .map_err(CasError::store)?;
        Ok(info.map(|t| t.hash_and_format().hash))
    }

    /// Return all named pins.
    pub async fn list_pins(&self) -> Result<Vec<PinInfo>> {
        use futures_lite::StreamExt as _;

        let mut stream = self
            .inner
            .tags()
            .list()
            .await
            .map_err(CasError::store)?;

        let mut pins = Vec::new();
        while let Some(item) = stream.next().await {
            pins.push(item.map_err(CasError::store)?);
        }
        Ok(pins)
    }

    // ── Listing ───────────────────────────────────────────────────────────────

    /// Return the [`BlobId`]s of all fully-stored blobs.
    pub async fn list(&self) -> Result<Vec<BlobId>> {
        self.inner
            .blobs()
            .list()
            .hashes()
            .await
            .map_err(CasError::store)
    }

    // ── Batch (high-throughput bulk ingestion) ──────────────────────────────

    /// Open a [`CasBatch`] session for high-throughput bulk ingestion.
    ///
    /// Inside a batch, all blobs share a single actor scope so redb write
    /// transactions can be coalesced.  This dramatically reduces per-blob
    /// overhead when storing millions of small files.
    ///
    /// Blobs added to the batch are kept alive by the batch's own [`TempPin`]
    /// scope.  Call [`CasStore::pin`] to promote individual blobs to named
    /// pins before dropping the batch.
    pub async fn batch(&self) -> Result<CasBatch<'_>> {
        let batch = self
            .inner
            .blobs()
            .batch()
            .await
            .map_err(CasError::store)?;
        Ok(CasBatch { batch, store: self })
    }

    /// Convenience for storing many blobs concurrently.
    ///
    /// Spawns all writes inside a single [`CasBatch`] scope and returns a
    /// `Vec` of `(BlobId, input_index)` pairs in **completion order** (not
    /// input order).  Use the index to correlate results with your inputs.
    ///
    /// For ordered results just sort by the returned index afterward.
    pub async fn put_many(
        &self,
        items: impl IntoIterator<Item = impl Into<Bytes>>,
    ) -> Result<Vec<BlobId>> {
        let batch = self.batch().await?;
        let futs: Vec<_> = items
            .into_iter()
            .map(|data| batch.put_bytes(data.into()))
            .collect();

        let mut ids = Vec::with_capacity(futs.len());
        for fut in futs {
            ids.push(fut.await?);
        }
        Ok(ids)
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Flush in-memory metadata to disk.
    ///
    /// No-op for the memory store.  Call periodically or before a clean
    /// shutdown to avoid loss of recent writes on crash.
    pub async fn sync_db(&self) -> Result<()> {
        self.inner.sync_db().await.map_err(CasError::store)
    }

    /// Cleanly shut down the store.
    ///
    /// Ensures all pending writes are flushed before returning.  After this
    /// call the store (and all its clones) are inoperable.
    pub async fn shutdown(self) -> Result<()> {
        self.inner.shutdown().await.map_err(CasError::store)
    }

    // ── Escape hatch ─────────────────────────────────────────────────────────

    /// Access the underlying iroh-blobs [`Store`] for capabilities not exposed
    /// by this API (e.g. BAO streaming, downloader, remote sync).
    pub fn inner(&self) -> &Store {
        &self.inner
    }
}

impl std::fmt::Debug for CasStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasStore").finish_non_exhaustive()
    }
}

// ─── CasBatch ────────────────────────────────────────────────────────────────

/// A batch ingestion session for high-throughput bulk writes.
///
/// All blobs written through this session share a single actor scope.
/// For FsStore this means redb write transactions are coalesced across blobs,
/// dramatically reducing per-blob overhead when ingesting millions of small
/// files.
///
/// Obtain via [`CasStore::batch`].  Drop when done; blobs are kept alive by
/// any [`TempPin`]s you obtained, or by named pins you created with
/// [`CasStore::pin`].
pub struct CasBatch<'store> {
    batch: Batch<'store>,
    store: &'store CasStore,
}

impl<'store> CasBatch<'store> {
    /// Store raw bytes within this batch.  Returns the [`BlobId`].
    pub async fn put_bytes(&self, data: impl Into<Bytes>) -> Result<BlobId> {
        let tt = self
            .batch
            .add_bytes(data.into())
            .temp_tag()
            .await
            .map_err(CasError::store)?;
        Ok(tt.hash())
    }

    /// Store a file by copying it into the store within this batch.
    pub async fn put_path(&self, path: impl AsRef<Path>) -> Result<BlobId> {
        use iroh_blobs::api::blobs::AddPathOptions;
        let tt = self
            .batch
            .add_path_with_opts(AddPathOptions {
                path: path.as_ref().to_owned(),
                mode: iroh_blobs::api::blobs::ImportMode::Copy,
                format: iroh_blobs::BlobFormat::Raw,
            })
            .temp_tag()
            .await
            .map_err(CasError::store)?;
        Ok(tt.hash())
    }

    /// Immediately promote a blob to a named pin within this batch.
    ///
    /// Equivalent to calling `store.pin(id, name)` but avoids an extra
    /// round-trip when the id is freshly computed.
    pub async fn pin(&self, id: BlobId, name: impl AsRef<[u8]>) -> Result<()> {
        self.store.pin(id, name).await
    }
}

impl std::fmt::Debug for CasBatch<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasBatch").finish_non_exhaustive()
    }
}
