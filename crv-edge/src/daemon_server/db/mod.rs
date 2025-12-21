pub mod active_file;
pub mod changelist;
pub mod config;
pub mod file;
pub mod workspace;

use bincode::{Decode, Encode};
use rocksdb::{ColumnFamilyDescriptor, IteratorMode, OptimisticTransactionDB, Options};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("RocksDB internal error: {0}")]
    RocksDb(#[from] rocksdb::Error),
    #[error("Encode error: {0}")]
    EncodeError(#[from] bincode::error::EncodeError),
    #[error("Decode error: {0}")]
    DecodeError(#[from] bincode::error::DecodeError),
    #[error("{0}")]
    WorkspaceConflict(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

/// 用于标记一个 key 是否真的已经写入完成了，用于需要分阶段的写入过程，如工作区创建
#[derive(Encode, Decode, PartialEq, Eq)]
pub enum Status {
    Pending,
    Confirmed,
}

/// 数据库管理器，负责持有 DB 句柄
pub struct DbManager {
    // 使用 Arc 让 DB 可以在多线程（gRPC handlers）间安全共享
    // rust-rocksdb 的 DB 本身是 Thread-safe 的
    inner: Arc<OptimisticTransactionDB>,
}

impl DbManager {
    // 定义列族名称常量
    const CF_APP_CONFIG: &'static str = "app_config";
    const CF_META_REVISION: &'static str = "meta_revision";
    const CF_WORKSPACE: &'static str = "workspace";
    const CF_FILE: &'static str = "file";
    const CF_CHANGELIST: &'static str = "changelist";
    const CF_ACTIVE_FILE: &'static str = "active_file";

    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self, DbError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let root: &Path = root.as_ref();
        let path = root.join("db.edge");

        // 定义所需的列族
        let cfs = vec![
            ColumnFamilyDescriptor::new(Self::CF_APP_CONFIG, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_WORKSPACE, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_META_REVISION, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_FILE, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_CHANGELIST, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_ACTIVE_FILE, Options::default()),
        ];

        let db = OptimisticTransactionDB::open_cf_descriptors(&opts, path, cfs)?;
        Ok(Self {
            inner: Arc::new(db),
        })
    }
}
