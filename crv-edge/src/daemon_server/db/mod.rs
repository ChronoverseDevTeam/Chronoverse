use rocksdb::{ColumnFamilyDescriptor, DB, Options};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

use crate::daemon_server::config::RuntimeConfig;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("RocksDB internal error: {0}")]
    RocksDb(#[from] rocksdb::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// 数据库管理器，负责持有 DB 句柄
pub struct DbManager {
    // 使用 Arc 让 DB 可以在多线程（gRPC handlers）间安全共享
    // rust-rocksdb 的 DB 本身是 Thread-safe 的
    inner: Arc<DB>,
}

impl DbManager {
    // 定义列族名称常量
    const CF_DEFAULT: &'static str = "default";
    const CF_APP_CONFIG: &'static str = "app_config";
    const CF_METADATA: &'static str = "metadata";

    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self, DbError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let root: &Path = root.as_ref();
        let path = root.join("db.edge");

        // 定义所需的列族
        let cfs = vec![
            ColumnFamilyDescriptor::new(Self::CF_DEFAULT, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_APP_CONFIG, Options::default()),
            ColumnFamilyDescriptor::new(Self::CF_METADATA, Options::default()),
        ];

        let db = DB::open_cf_descriptors(&opts, path, cfs)?;
        Ok(Self {
            inner: Arc::new(db),
        })
    }

    pub fn load_runtime_config(&self) -> Result<Option<RuntimeConfig>, DbError> {
        todo!()
    }

    /// 获取应用配置 (反序列化示例)
    fn get_config(&self, key: &str) -> Result<Option<String>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_APP_CONFIG)
            .expect("CF_APP_CONFIG must exist");

        match self.inner.get_cf(cf, key)? {
            Some(bytes) => {
                // 假设配置存的是 UTF-8 字符串，如果用 Protobuf，这里用 prost 解码
                let val =
                    String::from_utf8(bytes).map_err(|e| DbError::Serialization(e.to_string()))?;
                Ok(Some(val))
            }
            None => Ok(None),
        }
    }

    /// 写入应用配置
    fn set_config(&self, key: &str, value: &str) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_APP_CONFIG)
            .expect("CF_APP_CONFIG must exist");
        self.inner.put_cf(cf, key, value.as_bytes())?;
        Ok(())
    }
}
