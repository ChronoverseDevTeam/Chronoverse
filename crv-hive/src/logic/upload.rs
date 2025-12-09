use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use crv_core::repository::{ChunkHash, Compression, HASH_SIZE, RepositoryError, RepositoryManager, compute_chunk_hash};
use futures::StreamExt;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tonic::{Request, Response, Status};

use crate::config::holder::get_or_init_config;
use crate::pb::{FileChunk, NilRsp};

struct TempChunkMeta {
    path: PathBuf,
    total_len: u64,
}

pub async fn upload(
    request: Request<tonic::Streaming<FileChunk>>,
) -> Result<Response<NilRsp>, Status> {
    let config = get_or_init_config();
    let repo_root = PathBuf::from(&config.repository_path);
    let temp_dir = repo_root.join("temp");
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|err| map_io_error("创建临时目录失败", err))?;

    let mut stream = request.into_inner();
    let mut temp_chunks: HashMap<ChunkHash, TempChunkMeta> = HashMap::new();
    let mut temp_paths: Vec<PathBuf> = Vec::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(chunk) => chunk,
            Err(status) => {
                cleanup_paths(&temp_paths).await;
                return Err(Status::internal(format!(
                    "读取上传流失败: {}",
                    status.message()
                )));
            }
        };

        if chunk.chunk_hash.len() != HASH_SIZE {
            cleanup_paths(&temp_paths).await;
            return Err(Status::invalid_argument(format!(
                "chunk_hash 必须为 {HASH_SIZE} 字节"
            )));
        }
        let mut hash = [0u8; HASH_SIZE];
        hash.copy_from_slice(&chunk.chunk_hash);

        if chunk.offset < 0 {
            cleanup_paths(&temp_paths).await;
            return Err(Status::invalid_argument("chunk offset 不能为负数"));
        }
        let offset = chunk.offset as u64;
        let data = chunk.data;

        if !temp_chunks.contains_key(&hash) {
            let file_path = temp_dir.join(format!("{}.chunk.tmp", hash_to_hex(&hash)));
            if let Err(err) = fs::remove_file(&file_path).await {
                if err.kind() != std::io::ErrorKind::NotFound {
                    cleanup_paths(&temp_paths).await;
                    return Err(map_io_error("清理历史临时文件失败", err));
                }
            }
            if let Err(err) = File::create(&file_path).await {
                cleanup_paths(&temp_paths).await;
                return Err(map_io_error("创建临时 chunk 文件失败", err));
            }
            temp_paths.push(file_path.clone());
            temp_chunks.insert(
                hash,
                TempChunkMeta {
                    path: file_path,
                    total_len: 0,
                },
            );
        }

        let meta = temp_chunks
            .get_mut(&hash)
            .expect("chunk 元信息必须存在");
        if let Err(status) = write_fragment(&meta.path, offset, &data).await {
            cleanup_paths(&temp_paths).await;
            return Err(status);
        }
        meta.total_len = meta.total_len.max(offset + data.len() as u64);
    }

    if temp_chunks.is_empty() {
        return Err(Status::invalid_argument("未接收到任何 chunk 数据"));
    }

    let manager = match RepositoryManager::new(&repo_root) {
        Ok(m) => m,
        Err(err) => {
            cleanup_paths(&temp_paths).await;
            return Err(map_repository_error(err));
        }
    };

    for (hash, meta) in temp_chunks.iter() {
        let bytes = match fs::read(&meta.path).await {
            Ok(bytes) => bytes,
            Err(err) => {
                cleanup_paths(&temp_paths).await;
                return Err(map_io_error("读取临时 chunk 文件失败", err));
            }
        };
        let computed = compute_chunk_hash(&bytes);
        if computed != *hash {
            cleanup_paths(&temp_paths).await;
            return Err(Status::data_loss(format!(
                "chunk hash 不匹配，期望 {} 实际 {}",
                hash_to_hex(hash),
                hash_to_hex(&computed)
            )));
        }

        let persisted_hash = match manager.write_chunk(&bytes, Compression::None) {
            Ok(record) => record.hash,
            Err(RepositoryError::DuplicateHash { .. }) => *hash,
            Err(err) => {
                cleanup_paths(&temp_paths).await;
                return Err(map_repository_error(err));
            }
        };

        if persisted_hash != *hash {
            cleanup_paths(&temp_paths).await;
            return Err(Status::data_loss(format!(
                "入库 hash 不一致，期望 {} 实际 {}",
                hash_to_hex(hash),
                hash_to_hex(&persisted_hash)
            )));
        }

        let _ = fs::remove_file(&meta.path).await;
    }

    cleanup_paths(&temp_paths).await;
    Ok(Response::new(NilRsp {}))
}

async fn write_fragment(path: &Path, offset: u64, data: &[u8]) -> Result<(), Status> {
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .open(path)
        .await
        .map_err(|err| map_io_error("打开临时文件失败", err))?;
    file.seek(SeekFrom::Start(offset))
        .await
        .map_err(|err| map_io_error("定位临时文件失败", err))?;
    file.write_all(data)
        .await
        .map_err(|err| map_io_error("写入临时文件失败", err))?;
    Ok(())
}

async fn cleanup_paths(paths: &[PathBuf]) {
    for path in paths {
        let target = path.clone();
        let _ = fs::remove_file(target).await;
    }
}

fn hash_to_hex(hash: &ChunkHash) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn map_io_error(context: &str, err: std::io::Error) -> Status {
    Status::internal(format!("{context}: {err}"))
}

fn map_repository_error(err: RepositoryError) -> Status {
    match err {
        RepositoryError::DuplicateHash { .. } => {
            Status::already_exists("chunk 已存在于仓库中")
        }
        other => Status::internal(format!("仓库写入失败: {other}")),
    }
}