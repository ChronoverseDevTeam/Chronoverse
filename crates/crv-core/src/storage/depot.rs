use crv_shared::error::{CrvError, Result};
use sha2::{Sha256, Digest};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

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

    /// Compute SHA-256 digest of file content.
    pub fn compute_digest(content: &[u8]) -> String {
        hex::encode(Sha256::digest(content))
    }

    /// Get the path where a blob with the given digest would be stored.
    pub fn blob_path(&self, digest: &str) -> PathBuf {
        let (first, rest) = digest.split_at(2);
        self.root.join("blobs").join(first).join(format!("{rest}.blob"))
    }

    /// Store file content and return its digest.
    /// If the blob already exists (same content), it is not overwritten.
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

    /// Read the content of a stored blob by digest.
    pub async fn read_blob(&self, digest: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(digest);
        fs::read(&path)
            .await
            .map_err(|e| CrvError::Storage(format!("cannot read blob {digest}: {e}")))
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
