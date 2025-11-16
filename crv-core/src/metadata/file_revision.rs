use crate::storage::file_block::FileBlock;
use crate::storage::{ChunkingOptions, chunk_and_store_file};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

// Used in MongoDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MetaFileRevision {
    pub revision: u64,
    pub related_changelist_id: u64,
    pub block_hashes: Vec<String>,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
}

impl MetaFileRevision {
    /// Ingest a source file: chunk it, persist blocks under `store_root`, and build a revision.
    pub fn from_source_file<P: AsRef<Path>, Q: AsRef<Path>>(
        revision: u64,
        related_changelist_id: u64,
        source_file: P,
        store_root: Q,
        options: &ChunkingOptions,
    ) -> std::io::Result<Self> {
        let blocks: Vec<FileBlock> = chunk_and_store_file(&source_file, &store_root, options)?;
        let block_hashes = blocks.into_iter().map(|b| b.id).collect();
        let now = Utc::now();
        Ok(Self {
            revision,
            related_changelist_id,
            block_hashes,
            created_at: now,
            updated_at: now,
        })
    }

    /// Materialize this revision to a destination path by concatenating stored blocks.
    pub fn restore_to_path<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        store_root: P,
        dest_path: Q,
    ) -> std::io::Result<()> {
        if let Some(parent) = dest_path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(dest_path)?;
        for id in &self.block_hashes {
            let path = block_path_for_id(&store_root, id);
            let mut f = File::open(path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            out.write_all(&buf)?;
        }
        Ok(())
    }
}

fn block_path_for_id<P: AsRef<Path>>(store_root: P, id: &str) -> PathBuf {
    let mut dir = PathBuf::from(store_root.as_ref());
    let p1 = &id[0..2];
    let p2 = &id[2..4];
    let p3 = &id[4..6];
    dir.push(p1);
    dir.push(p2);
    dir.push(p3);
    let mut path = dir;
    path.push(id);
    path
}
