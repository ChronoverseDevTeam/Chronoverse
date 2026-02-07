use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::hive_server::repository_manager;
use crate::logging::HiveLog;
use crate::pb::{DownloadFileChunkReq, DownloadFileChunkResp};
use crv_core::repository::{blake3_hex_to_hash, RepositoryError};

pub type DownloadFileChunkStream = ReceiverStream<Result<DownloadFileChunkResp, Status>>;

pub async fn handle_download_file_chunk(
    log: HiveLog,
    request: Request<DownloadFileChunkReq>,
) -> Result<Response<DownloadFileChunkStream>, Status> {
    let _g = log.enter();
    let req = request.into_inner();
    let log_spawn = log.clone();

    if req.chunk_hashes.is_empty() {
        return Err(Status::invalid_argument("chunk_hashes is empty"));
    }

    let repo = repository_manager()?;
    let (tx, rx) = mpsc::channel::<Result<DownloadFileChunkResp, Status>>(32);

    const MAX_PACKET_SIZE: usize = 4 * 1024 * 1024;
    let mut packet_size = if req.packet_size > 0 {
        req.packet_size
    } else {
        MAX_PACKET_SIZE as i64
    };
    if packet_size as usize > MAX_PACKET_SIZE {
        packet_size = MAX_PACKET_SIZE as i64;
    }
    let packet_size = packet_size
        .try_into()
        .unwrap_or(MAX_PACKET_SIZE);

    tokio::spawn(async move {
        let _g = log_spawn.enter();
        log_spawn.info("download_file_chunk stream started");

        for chunk_hash in req.chunk_hashes {
            let hash_bytes = match blake3_hex_to_hash(&chunk_hash) {
                Some(h) => h,
                None => {
                    let _ = tx
                        .send(Err(Status::invalid_argument(format!(
                            "invalid chunk_hash: {}",
                            chunk_hash
                        ))))
                        .await;
                    break;
                }
            };

            let data = match repo.read_chunk(&hash_bytes) {
                Ok(data) => data,
                Err(RepositoryError::ChunkNotFound { .. }) => {
                    let _ = tx
                        .send(Err(Status::not_found(format!(
                            "chunk not found: {}",
                            chunk_hash
                        ))))
                        .await;
                    break;
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(Status::internal(format!(
                            "read chunk failed: {}",
                            e
                        ))))
                        .await;
                    break;
                }
            };

            let total_len = data.len();
            let total_len_u64 = total_len as u64;
            let total_len_u32 = total_len as u32;

            if total_len == 0 {
                let rsp = DownloadFileChunkResp {
                    chunk_hash: chunk_hash.clone(),
                    offset: 0,
                    content: Vec::new(),
                    size: total_len_u64,
                    compression: "none".to_string(),
                    uncompressed_size: total_len_u32,
                };
                if tx.send(Ok(rsp)).await.is_err() {
                    break;
                }
                continue;
            }

            let mut offset: usize = 0;
            while offset < total_len {
                let end = (offset + packet_size).min(total_len);
                let content = data[offset..end].to_vec();
                let offset_i64 = match i64::try_from(offset) {
                    Ok(v) => v,
                    Err(_) => {
                        let _ = tx
                            .send(Err(Status::internal("offset overflow")))
                            .await;
                        return;
                    }
                };

                let rsp = DownloadFileChunkResp {
                    chunk_hash: chunk_hash.clone(),
                    offset: offset_i64,
                    content,
                    size: total_len_u64,
                    compression: "none".to_string(),
                    uncompressed_size: total_len_u32,
                };

                if tx.send(Ok(rsp)).await.is_err() {
                    return;
                }
                offset = end;
            }
        }

        log_spawn.finish_ok();
    });

    Ok(Response::new(ReceiverStream::new(rx)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{entity::ConfigEntity, holder::try_set_config};
    use crate::logging::init_logging;
    use crv_core::repository::{blake3_hash_to_hex, compute_chunk_hash, Compression, RepositoryError};
    use std::sync::OnceLock;
    use tokio_stream::StreamExt;

    static TEST_INIT: OnceLock<()> = OnceLock::new();
    static TEST_REPO_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    static TEST_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    fn test_mutex() -> &'static tokio::sync::Mutex<()> {
        TEST_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    fn init_test_repo() {
        TEST_INIT.get_or_init(|| {
            init_logging();
            let dir = tempfile::tempdir().expect("create temp dir");
            let repo_root = dir.path().join("repo");
            let cache_root = dir.path().join("cache");

            let mut cfg = ConfigEntity::default();
            cfg.repository_path = repo_root.to_string_lossy().into_owned();
            cfg.upload_cache_path = cache_root.to_string_lossy().into_owned();

            let _ = try_set_config(cfg);
            let _ = TEST_REPO_DIR.set(dir);
        });
    }

    #[tokio::test]
    async fn test_download_single_chunk_split() {
        let _g = test_mutex().lock().await;
        init_test_repo();

        let repo = crate::hive_server::repository_manager().expect("repository_manager");
        let data = b"hello download chunk";
        let hash = compute_chunk_hash(data);
        let hash_hex = blake3_hash_to_hex(&hash);

        match repo.write_chunk(data, Compression::None) {
            Ok(_) => {}
            Err(RepositoryError::DuplicateHash { .. }) => {}
            Err(e) => panic!("write_chunk failed: {e}"),
        }

        let req = DownloadFileChunkReq {
            chunk_hashes: vec![hash_hex.clone()],
            packet_size: 4,
        };

        let log = HiveLog::new("DownloadFileChunk(test_download_single_chunk_split)");
        let resp = handle_download_file_chunk(log, Request::new(req))
            .await
            .expect("handle_download_file_chunk");
        let mut stream = resp.into_inner();

        let mut parts: Vec<DownloadFileChunkResp> = Vec::new();
        while let Some(item) = stream.next().await {
            parts.push(item.expect("stream item ok"));
        }

        assert!(!parts.is_empty());
        let mut rebuilt: Vec<u8> = Vec::new();
        let mut last_offset = -1;
        for p in &parts {
            assert_eq!(p.chunk_hash, hash_hex);
            assert_eq!(p.size, data.len() as u64);
            assert_eq!(p.compression, "none");
            assert_eq!(p.uncompressed_size, data.len() as u32);
            assert!(p.offset > last_offset);
            last_offset = p.offset;
            rebuilt.extend_from_slice(&p.content);
        }
        assert_eq!(rebuilt, data);
    }

    #[tokio::test]
    async fn test_download_invalid_chunk_hash() {
        let _g = test_mutex().lock().await;
        init_test_repo();

        let req = DownloadFileChunkReq {
            chunk_hashes: vec!["invalid-hash".to_string()],
            packet_size: 0,
        };
        let log = HiveLog::new("DownloadFileChunk(test_download_invalid_chunk_hash)");
        let resp = handle_download_file_chunk(log, Request::new(req))
            .await
            .expect("stream should be opened");
        let mut stream = resp.into_inner();

        let first = stream.next().await.expect("first item exists");
        let err = first.err().expect("should be error");
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_download_empty_hashes() {
        let _g = test_mutex().lock().await;
        init_test_repo();

        let req = DownloadFileChunkReq {
            chunk_hashes: vec![],
            packet_size: 0,
        };
        let log = HiveLog::new("DownloadFileChunk(test_download_empty_hashes)");
        let err = handle_download_file_chunk(log, Request::new(req))
            .await
            .expect_err("should reject empty chunk_hashes");
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_download_large_chunk_17mb() {
        let _g = test_mutex().lock().await;
        init_test_repo();

        let repo = crate::hive_server::repository_manager().expect("repository_manager");
        let data = vec![0xABu8; 17 * 1024 * 1024];
        let hash = compute_chunk_hash(&data);
        let hash_hex = blake3_hash_to_hex(&hash);

        match repo.write_chunk(&data, Compression::None) {
            Ok(_) => {}
            Err(RepositoryError::DuplicateHash { .. }) => {}
            Err(e) => panic!("write_chunk failed: {e}"),
        }

        let req = DownloadFileChunkReq {
            chunk_hashes: vec![hash_hex.clone()],
            packet_size: (4 * 1024 * 1024) as i64,
        };

        let log = HiveLog::new("DownloadFileChunk(test_download_large_chunk_17mb)");
        let resp = handle_download_file_chunk(log, Request::new(req))
            .await
            .expect("handle_download_file_chunk");
        let mut stream = resp.into_inner();

        let mut total = 0usize;
        let mut offsets = Vec::new();
        let mut parts = 0usize;
        while let Some(item) = stream.next().await {
            let rsp = item.expect("stream item ok");
            assert_eq!(rsp.chunk_hash, hash_hex);
            assert_eq!(rsp.size, data.len() as u64);
            assert_eq!(rsp.compression, "none");
            assert_eq!(rsp.uncompressed_size, data.len() as u32);
            offsets.push(rsp.offset);
            total += rsp.content.len();
            parts += 1;
        }

        assert_eq!(total, data.len());
        assert!(parts >= 5, "17MB should be split into multiple parts");
        assert_eq!(offsets.first().copied().unwrap_or(-1), 0);
    }

    #[tokio::test]
    async fn test_download_multiple_chunks_in_one_request() {
        let _g = test_mutex().lock().await;
        init_test_repo();

        let repo = crate::hive_server::repository_manager().expect("repository_manager");
        let data_a = b"chunk-a-data".to_vec();
        let data_b = b"chunk-b-data".to_vec();

        let hash_a = compute_chunk_hash(&data_a);
        let hash_b = compute_chunk_hash(&data_b);
        let hash_a_hex = blake3_hash_to_hex(&hash_a);
        let hash_b_hex = blake3_hash_to_hex(&hash_b);

        match repo.write_chunk(&data_a, Compression::None) {
            Ok(_) => {}
            Err(RepositoryError::DuplicateHash { .. }) => {}
            Err(e) => panic!("write_chunk A failed: {e}"),
        }
        match repo.write_chunk(&data_b, Compression::None) {
            Ok(_) => {}
            Err(RepositoryError::DuplicateHash { .. }) => {}
            Err(e) => panic!("write_chunk B failed: {e}"),
        }

        let req = DownloadFileChunkReq {
            chunk_hashes: vec![hash_a_hex.clone(), hash_b_hex.clone()],
            packet_size: 0,
        };

        let log = HiveLog::new("DownloadFileChunk(test_download_multiple_chunks_in_one_request)");
        let resp = handle_download_file_chunk(log, Request::new(req))
            .await
            .expect("handle_download_file_chunk");
        let mut stream = resp.into_inner();

        let mut got_a = Vec::new();
        let mut got_b = Vec::new();

        while let Some(item) = stream.next().await {
            let rsp = item.expect("stream item ok");
            if rsp.chunk_hash == hash_a_hex {
                got_a.extend_from_slice(&rsp.content);
            } else if rsp.chunk_hash == hash_b_hex {
                got_b.extend_from_slice(&rsp.content);
            } else {
                panic!("unexpected chunk_hash: {}", rsp.chunk_hash);
            }
        }

        assert_eq!(got_a, data_a);
        assert_eq!(got_b, data_b);
    }
}