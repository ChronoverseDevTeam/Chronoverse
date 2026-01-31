use tonic::{Request, Response, Status};

use crate::{
    hive_server::submit::{submit_service, cache_service, submit::UploadFileChunkStream},
    pb::{UploadFileChunkReq, UploadFileChunkRsp},
};

fn spawn_upload_file_chunk_handler<S>(mut req: S) -> UploadFileChunkStream
where
    S: tokio_stream::Stream<Item = Result<UploadFileChunkReq, Status>> + Send + Unpin + 'static,
{
    use std::collections::HashSet;
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_stream::StreamExt;

    let (tx, rx) = mpsc::channel::<Result<UploadFileChunkRsp, Status>>(32);

    tokio::spawn(async move {
        // 一个 stream 里可能包含多个不同的 chunk（由 chunks_amount 指示总数）
        let mut expected_chunks_amount: Option<usize> = None;
        // 已经对某个 chunk_hash 发送过"最终响应"的集合，保证每个 chunk 只回应一次
        let mut responded_chunks: HashSet<String> = HashSet::new();

        while let Some(item) = req.next().await {
            let item = match item {
                Ok(item) => item,
                Err(e) => {
                    let _ = tx
                        .send(Err(Status::internal(format!("stream error: {}", e))))
                        .await;
                    break;
                }
            };

            // 解析 ticket
            let ticket_uuid = match uuid::Uuid::parse_str(&item.ticket) {
                Ok(uuid) => uuid,
                Err(e) => {
                    let _ = tx
                        .send(Err(Status::invalid_argument(format!(
                            "invalid ticket format: {}",
                            e
                        ))))
                        .await;
                    break; // 遇到错误请求，直接断开连接
                }
            };

            // 验证并设置预期的 chunk 数量（只能设置一次）
            if let Some(expected_amount) = expected_chunks_amount {
                if item.chunks_amount > 0 && expected_amount != item.chunks_amount as usize {
                    let _ = tx
                        .send(Err(Status::invalid_argument(format!(
                            "chunks_amount mismatch: expected {}, got {}",
                            expected_amount, item.chunks_amount
                        ))))
                        .await;
                    break;
                }
            } else if item.chunks_amount > 0 {
                expected_chunks_amount = Some(item.chunks_amount as usize);
            }

            // 检查是否已经对该 chunk 发送过最终响应
            if responded_chunks.contains(&item.chunk_hash) {
                let _ = tx
                    .send(Err(Status::invalid_argument(format!(
                        "duplicate chunk_hash: {}",
                        item.chunk_hash
                    ))))
                    .await;
                break;
            }

            // 检查 chunk 是否已经完整存在（秒传优化）
            // 只在第一个数据包（offset=0）时检查，避免重复检查
            if item.offset == 0 {
                let cache = cache_service();
                match cache.has_chunk(&item.chunk_hash) {
                    Ok(true) => {
                        responded_chunks.insert(item.chunk_hash.clone());

                        let rsp = UploadFileChunkRsp {
                            ticket: item.ticket.clone(),
                            success: true,
                            chunk_hash: item.chunk_hash.clone(),
                            message: "chunk already exists".to_string(),
                            already_exists: true,
                        };

                        if tx.send(Ok(rsp)).await.is_err() {
                            break;
                        }
                        continue;
                    }
                    Ok(false) => {
                        // 正常情况：不存在则继续上传
                    }
                    Err(e) => {
                        let error_msg = match e {
                            crate::caching::ChunkCacheError::InvalidChunkHash(msg) => {
                                format!("invalid chunk hash during check: {}", msg)
                            }
                            crate::caching::ChunkCacheError::Io(io_err) => {
                                format!("io error during chunk check: {}", io_err)
                            }
                            crate::caching::ChunkCacheError::HashMismatch { expected, actual } => {
                                format!(
                                    "chunk hash mismatch during check: expected {}, actual {}",
                                    expected, actual
                                )
                            }
                        };
                        let _ = tx.send(Err(Status::internal(error_msg))).await;
                        break;
                    }
                }
            }

            // 调用 submit_service 处理 chunk 上传
            let service = submit_service();
            match service.upload_file_chunk(
                &ticket_uuid,
                &item.chunk_hash,
                item.offset,
                item.chunk_size,
                &item.content,
            ) {
                Ok(result) => {
                    use crate::hive_server::submit::service::UploadFileChunkResult;
                    match result {
                        UploadFileChunkResult::FileUploadFinished => {
                            responded_chunks.insert(item.chunk_hash.clone());

                            let rsp = UploadFileChunkRsp {
                                ticket: item.ticket.clone(),
                                success: true,
                                chunk_hash: item.chunk_hash.clone(),
                                message: "chunk uploaded successfully".to_string(),
                                already_exists: false,
                            };

                            if tx.send(Ok(rsp)).await.is_err() {
                                break;
                            }
                        }
                        UploadFileChunkResult::FileAppended => {
                            // 未完成不回应
                        }
                    }
                }
                Err(e) => {
                    let rsp = UploadFileChunkRsp {
                        ticket: item.ticket.clone(),
                        success: false,
                        chunk_hash: item.chunk_hash.clone(),
                        message: e.message.clone(),
                        already_exists: false,
                    };

                    let _ = tx.send(Ok(rsp)).await;
                    break;
                }
            }
        }
    });

    ReceiverStream::new(rx)
}

pub fn upload_file_chunk(
    r: Request<tonic::Streaming<UploadFileChunkReq>>,
) -> Result<Response<UploadFileChunkStream>, Status> {
    let req = r.into_inner();
    Ok(Response::new(spawn_upload_file_chunk_handler(req)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;
    use tokio_stream::StreamExt;

    static TEST_CACHE_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    static TEST_INIT: OnceLock<()> = OnceLock::new();

    fn init_test_globals() {
        TEST_INIT.get_or_init(|| {
            let dir = tempfile::tempdir().expect("create temp dir");
            let cache_root = dir.path().join("cache");

            let cache = crate::caching::ChunkCache::new(&cache_root).expect("create cache");
            let _ = crate::hive_server::submit::CACHE_SERVICE.set(cache);

            let _ = crate::hive_server::submit::SUBMIT_SERVICE
                .set(crate::hive_server::submit::service::SubmitService::new());

            // keep tempdir alive for entire test process
            let _ = TEST_CACHE_DIR.set(dir);
        });
    }

    fn ensure_ticket(ticket: uuid::Uuid) {
        let svc = crate::hive_server::submit::submit_service();
        svc.insert_test_context(ticket);
    }

    fn compute_chunk_hash(data: &[u8]) -> String {
        use crv_core::repository::compute_chunk_hash;
        let hash = compute_chunk_hash(data);
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    #[tokio::test]
    async fn test_upload_single_chunk_success() {
        init_test_globals();
        let ticket = uuid::Uuid::new_v4();
        ensure_ticket(ticket);

        let data = b"hello world";
        let chunk_hash = compute_chunk_hash(data);
        let chunk_size = data.len() as i64;

        let req = UploadFileChunkReq {
            ticket: ticket.to_string(),
            chunks_amount: 1,
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            chunk_size,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        };

        let input = tokio_stream::iter(vec![Ok(req)]);
        let mut stream = spawn_upload_file_chunk_handler(input);
        let mut responses = Vec::new();
        
        while let Some(result) = stream.next().await {
            match result {
                Ok(rsp) => responses.push(rsp),
                Err(e) => panic!("unexpected error: {}", e),
            }
        }

        assert_eq!(responses.len(), 1);
        let rsp = &responses[0];
        assert!(rsp.success);
        assert_eq!(rsp.chunk_hash, chunk_hash);
        assert!(!rsp.already_exists);
    }

    #[tokio::test]
    async fn test_upload_multiple_chunks_success() {
        init_test_globals();
        let ticket = uuid::Uuid::new_v4();
        ensure_ticket(ticket);

        let data1 = b"chunk 1";
        let data2 = b"chunk 2";
        let chunk_hash1 = compute_chunk_hash(data1);
        let chunk_hash2 = compute_chunk_hash(data2);

        let reqs = vec![
            UploadFileChunkReq {
                ticket: ticket.to_string(),
                chunks_amount: 2,
                chunk_hash: chunk_hash1.clone(),
                offset: 0,
                content: data1.to_vec(),
                chunk_size: data1.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data1.len() as u32,
            },
            UploadFileChunkReq {
                ticket: ticket.to_string(),
                chunks_amount: 2,
                chunk_hash: chunk_hash2.clone(),
                offset: 0,
                content: data2.to_vec(),
                chunk_size: data2.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data2.len() as u32,
            },
        ];

        let input = tokio_stream::iter(reqs.into_iter().map(Ok));
        let mut stream = spawn_upload_file_chunk_handler(input);
        let mut responses = Vec::new();
        
        while let Some(result) = stream.next().await {
            match result {
                Ok(rsp) => responses.push(rsp),
                Err(e) => panic!("unexpected error: {}", e),
            }
        }

        assert_eq!(responses.len(), 2);
        assert!(responses.iter().all(|r| r.success));
    }

    #[tokio::test]
    async fn test_invalid_ticket_format() {
        init_test_globals();

        let req = UploadFileChunkReq {
            ticket: "invalid-ticket".to_string(),
            chunks_amount: 1,
            chunk_hash: "abc123".to_string(),
            offset: 0,
            content: vec![1, 2, 3],
            chunk_size: 3,
            compression: "none".to_string(),
            uncompressed_size: 3,
        };

        let input = tokio_stream::iter(vec![Ok(req)]);
        let mut stream = spawn_upload_file_chunk_handler(input);
        let result = stream.next().await;
        
        assert!(result.is_some());
        match result.unwrap() {
            Ok(_) => panic!("should return error for invalid ticket"),
            Err(e) => {
                assert_eq!(e.code(), tonic::Code::InvalidArgument);
            }
        }
    }

    #[tokio::test]
    async fn test_chunks_amount_mismatch() {
        init_test_globals();
        let ticket = uuid::Uuid::new_v4();
        ensure_ticket(ticket);

        let data = b"test";
        let chunk_hash = compute_chunk_hash(data);

        let reqs = vec![
            UploadFileChunkReq {
                ticket: ticket.to_string(),
                chunks_amount: 2,
                chunk_hash: chunk_hash.clone(),
                offset: 0,
                content: data.to_vec(),
                chunk_size: data.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data.len() as u32,
            },
            UploadFileChunkReq {
                ticket: ticket.to_string(),
                chunks_amount: 3, // 不匹配
                chunk_hash: chunk_hash.clone(),
                offset: 0,
                content: data.to_vec(),
                chunk_size: data.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data.len() as u32,
            },
        ];

        let input = tokio_stream::iter(reqs.into_iter().map(Ok));
        let mut stream = spawn_upload_file_chunk_handler(input);
        let mut responses: Vec<UploadFileChunkRsp> = Vec::new();
        let mut errors: Vec<tonic::Status> = Vec::new();
        
        // 收集所有响应和错误
        while let Some(result) = stream.next().await {
            match result {
                Ok(rsp) => responses.push(rsp),
                Err(e) => {
                    errors.push(e);
                    break; // 遇到错误后断开连接
                }
            }
        }
        
        // 应该有一个错误（chunks_amount 不匹配）
        assert!(!errors.is_empty() || responses.iter().any(|r| !r.success));
        if let Some(err) = errors.first() {
            assert_eq!(err.code(), tonic::Code::InvalidArgument);
            assert!(err.message().contains("chunks_amount mismatch"));
        }
    }

    #[tokio::test]
    async fn test_duplicate_chunk_hash() {
        init_test_globals();
        let ticket = uuid::Uuid::new_v4();
        ensure_ticket(ticket);

        let data = b"test";
        let chunk_hash = compute_chunk_hash(data);

        // 预先写入 cache，确保第一个请求走 already_exists 分支（避免受 SubmitService/context 影响）
        {
            let cache = crate::hive_server::submit::cache_service();
            let _ = cache.remove_chunk(&chunk_hash);
            cache
                .append_chunk_part(&chunk_hash, 0, data)
                .expect("append chunk for already_exists");
            assert!(cache.has_chunk(&chunk_hash).expect("has_chunk") == true);
        }

        let reqs = vec![
            UploadFileChunkReq {
                ticket: ticket.to_string(),
                chunks_amount: 1,
                chunk_hash: chunk_hash.clone(),
                offset: 0,
                content: data.to_vec(),
                chunk_size: data.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data.len() as u32,
            },
            UploadFileChunkReq {
                ticket: ticket.to_string(),
                chunks_amount: 1,
                chunk_hash: chunk_hash.clone(), // 重复的 chunk_hash
                offset: 0,
                content: data.to_vec(),
                chunk_size: data.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data.len() as u32,
            },
        ];

        let input = tokio_stream::iter(reqs.into_iter().map(Ok));
        let mut stream = spawn_upload_file_chunk_handler(input);

        let first = stream.next().await.expect("first item exists");
        assert!(first.is_ok(), "first should be ok");
        let first_rsp = first.unwrap();
        assert!(first_rsp.success, "first should be success");
        assert!(first_rsp.already_exists, "first should be already_exists");

        let second = stream.next().await.expect("second item exists");
        assert!(second.is_err(), "second should be err");
        let e = second.err().unwrap();
        assert_eq!(e.code(), tonic::Code::InvalidArgument);
        assert!(e.message().contains("duplicate chunk_hash"));
    }

    #[tokio::test]
    async fn test_chunk_already_exists() {
        init_test_globals();
        let ticket = uuid::Uuid::new_v4();
        // already_exists 分支不依赖 context，但保持一致性
        ensure_ticket(ticket);

        let data = b"existing chunk";
        let chunk_hash = compute_chunk_hash(data);

        // 先上传一个 chunk
        let cache = crate::hive_server::submit::cache_service();
        cache
            .append_chunk_part(&chunk_hash, 0, data)
            .expect("append chunk");

        // 验证 chunk 存在
        assert!(cache.has_chunk(&chunk_hash).expect("check chunk") == true);

        // 尝试再次上传相同的 chunk
        let req = UploadFileChunkReq {
            ticket: ticket.to_string(),
            chunks_amount: 1,
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            chunk_size: data.len() as i64,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        };

        let input = tokio_stream::iter(vec![Ok(req)]);
        let mut stream = spawn_upload_file_chunk_handler(input);
        let result = stream.next().await;
        
        assert!(result.is_some());
        match result.unwrap() {
            Ok(rsp) => {
                assert!(rsp.success);
                assert!(rsp.already_exists);
                assert_eq!(rsp.chunk_hash, chunk_hash);
            }
            Err(e) => panic!("unexpected error: {}", e),
        }
    }
}
