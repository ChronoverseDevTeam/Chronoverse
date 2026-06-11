use crv_shared::error::{CrvError, Result};
use sha2::{Sha256, Digest};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Manages physical file storage under the depot root.
///
/// Layout:
/// ```text
/// {depot_root}/
///   blobs/
///     {first_2_hex}/
///       {rest_of_sha256}.blob
/// ```
///
/// Each file revision is stored as a content-addressed blob.
/// The `file_revisions` table maps depot_path + revision → digest.
pub struct Depot {
    root: PathBuf,
}

impl Depot {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Ensure the depot directory structure exists.
    pub async fn init(&self) -> Result<()> {
        let blobs = self.root.join("blobs");
        fs::create_dir_all(&blobs)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot create depot dir: {e}")))?;
        Ok(())
    }

    /// Compute SHA-256 digest of file content (in-memory, for small files).
    pub fn compute_digest(content: &[u8]) -> String {
        hex::encode(Sha256::digest(content))
    }

    /// Compute SHA-256 digest by reading a file incrementally.
    /// Works for files of any size without loading into memory.
    pub async fn compute_digest_from_file(&self, path: impl AsRef<Path>) -> Result<String> {
        let mut file = fs::File::open(path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot open for digest: {e}")))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024]; // 64 KB chunks
        loop {
            let n = file.read(&mut buf)
                .await
                .map_err(|e| CrvError::Storage(format!("read for digest: {e}")))?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    /// Get the path where a blob with the given digest would be stored.
    pub fn blob_path(&self, digest: &str) -> PathBuf {
        let (first, rest) = digest.split_at(2);
        self.root.join("blobs").join(first).join(format!("{rest}.blob"))
    }

    /// Store file content (in-memory) and return its digest.
    /// If the blob already exists, it is not overwritten.
    pub async fn store_blob(&self, content: &[u8]) -> Result<String> {
        let digest = Self::compute_digest(content);
        let path = self.blob_path(&digest);

        if path.exists() {
            return Ok(digest);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| CrvError::Storage(format!("cannot create blob dir: {e}")))?;
        }

        let mut file = fs::File::create(&path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot create blob file: {e}")))?;

        file.write_all(content)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot write blob: {e}")))?;

        Ok(digest)
    }

    /// Store file content from a stream (incremental hashing + writing).
    /// Suitable for files of any size. Returns the digest.
    pub async fn store_blob_stream(
        &self,
        reader: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> Result<String> {
        // Phase 1: read to a temp file while hashing
        let tmp = self.root.join("blobs").join(".tmp_streaming_blob");
        if let Some(parent) = tmp.parent() {
            fs::create_dir_all(parent).await
                .map_err(|e| CrvError::Storage(format!("cannot create tmp dir: {e}")))?;
        }

        let mut file = fs::File::create(&tmp)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot create tmp file: {e}")))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024]; // 64 KB

        loop {
            let n = reader.read(&mut buf)
                .await
                .map_err(|e| CrvError::Storage(format!("stream read: {e}")))?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .await
                .map_err(|e| CrvError::Storage(format!("stream write: {e}")))?;
        }

        let digest = hex::encode(hasher.finalize());
        let final_path = self.blob_path(&digest);

        // If blob already exists, delete temp; otherwise move temp to final
        if final_path.exists() {
            fs::remove_file(&tmp).await.ok();
        } else {
            if let Some(parent) = final_path.parent() {
                fs::create_dir_all(parent).await
                    .map_err(|e| CrvError::Storage(format!("cannot create blob dir: {e}")))?;
            }
            fs::rename(&tmp, &final_path)
                .await
                .map_err(|e| CrvError::Storage(format!("cannot rename blob: {e}")))?;
        }

        Ok(digest)
    }

    /// Read the content of a stored blob by digest (in-memory, for small files).
    pub async fn read_blob(&self, digest: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(digest);
        fs::read(&path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot read blob {digest}: {e}")))
    }

    /// Open a blob for streaming read. Returns a file handle.
    pub async fn read_blob_stream(&self, digest: &str) -> Result<tokio::fs::File> {
        let path = self.blob_path(digest);
        fs::File::open(&path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot open blob {digest}: {e}")))
    }

    /// Return the file size of a stored blob.
    pub async fn blob_size(&self, digest: &str) -> Result<u64> {
        let path = self.blob_path(digest);
        let meta = fs::metadata(&path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot stat blob {digest}: {e}")))?;
        Ok(meta.len())
    }

    /// Check if a blob exists.
    pub fn blob_exists(&self, digest: &str) -> bool {
        self.blob_path(digest).exists()
    }

    /// Delete a blob (used for obliterate/cleanup). Returns true if deleted.
    pub async fn delete_blob(&self, digest: &str) -> Result<bool> {
        let path = self.blob_path(digest);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(&path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot delete blob {digest}: {e}")))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_store_and_read_blob() {
        let dir = TempDir::new().unwrap();
        let depot = Depot::new(dir.path());
        depot.init().await.unwrap();

        let content = b"hello world v1";
        let digest = depot.store_blob(content).await.unwrap();
        assert_eq!(digest, Depot::compute_digest(content));

        let read = depot.read_blob(&digest).await.unwrap();
        assert_eq!(read, content);
    }

    #[tokio::test]
    async fn test_deduplication() {
        let dir = TempDir::new().unwrap();
        let depot = Depot::new(dir.path());
        depot.init().await.unwrap();

        let content = b"same content";
        let d1 = depot.store_blob(content).await.unwrap();
        let d2 = depot.store_blob(content).await.unwrap();
        assert_eq!(d1, d2, "same content should produce same digest");
    }

    #[tokio::test]
    async fn test_delete_blob() {
        let dir = TempDir::new().unwrap();
        let depot = Depot::new(dir.path());
        depot.init().await.unwrap();

        let digest = depot.store_blob(b"tmp").await.unwrap();
        assert!(depot.blob_exists(&digest));
        assert!(depot.delete_blob(&digest).await.unwrap());
        assert!(!depot.blob_exists(&digest));
    }
}
