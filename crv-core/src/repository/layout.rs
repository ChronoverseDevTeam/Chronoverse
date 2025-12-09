use std::fs;
use std::path::{Path, PathBuf};

use super::constants::{PACK_DATA_SUFFIX, PACK_FILE_PREFIX, PACK_INDEX_SUFFIX, SHARD_DIR_PREFIX};
use super::error::Result;
use super::io_utils::ensure_parent_dir;

pub struct RepositoryLayout {
    root: PathBuf,
}

impl RepositoryLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_shard_dir(&self, shard: u8) -> Result<PathBuf> {
        let dir = self.root.join(Self::shard_dir_name(shard));
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        Ok(dir)
    }

    pub fn shard_dir_name(shard: u8) -> String {
        format!("{SHARD_DIR_PREFIX}{shard:02x}")
    }

    pub fn pack_base_name(pack_id: u32) -> String {
        format!("{PACK_FILE_PREFIX}{pack_id:06}")
    }

    pub fn pack_paths(&self, shard: u8, pack_id: u32) -> Result<(PathBuf, PathBuf)> {
        let dir = self.ensure_shard_dir(shard)?;
        let base = Self::pack_base_name(pack_id);
        let dat_path = dir.join(format!("{base}{PACK_DATA_SUFFIX}"));
        let idx_path = dir.join(format!("{base}{PACK_INDEX_SUFFIX}"));
        ensure_parent_dir(&dat_path)?;
        ensure_parent_dir(&idx_path)?;
        Ok((dat_path, idx_path))
    }
}
