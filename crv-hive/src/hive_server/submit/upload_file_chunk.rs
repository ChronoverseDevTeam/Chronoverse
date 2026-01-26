use tonic::{Request, Response, Status};

use crate::{
    caching::ChunkCacheError,
    hive_server::submit::{cache_service, submit::UploadFileChunkStream},
    pb::{UploadFileChunkReq, UploadFileChunkRsp},
};

pub fn upload_file_chunk(
    r: Request<tonic::Streaming<UploadFileChunkReq>>,
) -> Result<Response<UploadFileChunkStream>, Status> {
    let mut req = r.into_inner();

    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    let (tx, rx) = mpsc::channel::<Result<UploadFileChunkRsp, Status>>(32);

    tokio::spawn(async move {
        use std::collections::HashSet;

        // 一个 stream 里可能包含多个不同的 chunk（由 chunks_amount 指示总数）
        let mut expected_chunks_amount: Option<usize> = None;
        // 已经对某个 chunk_hash 发送过“最终响应”的集合，保证每个 chunk 只回应一次
        let mut responded_chunks: HashSet<String> = HashSet::new();

        loop {
            let item = match req.message().await {
                Ok(Some(v)) => v,
                Ok(None) => break,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            };

            let ticket = item.ticket.clone();
            let chunk_hash = item.chunk_hash.trim().to_string();
            let offset = item.offset.max(0) as u64;
            let content = item.content;
            let chunk_size = item.chunk_size.max(0) as u64;
            let compression = item.compression.trim().to_string();
            let chunks_amount = item.chunks_amount.max(0) as u64;

            if expected_chunks_amount.is_none() && chunks_amount > 0 {
                expected_chunks_amount = Some(chunks_amount as usize);
            }

            // 如果本次批量的 chunk 都已回应，则直接结束（不额外推送“完成”消息）
            if let Some(total) = expected_chunks_amount {
                if total > 0 && responded_chunks.len() >= total {
                    break;
                }
            }

            // 当前服务端只支持不压缩写入缓存。
            if !compression.is_empty() && compression != "none" {
                // 只在该 chunk 尚未回应过时，发送一次最终失败响应
                if !responded_chunks.contains(&chunk_hash) {
                    let rsp = UploadFileChunkRsp {
                        ticket,
                        success: false,
                        chunk_hash: chunk_hash.clone(),
                        message: format!("unsupported compression: {compression}"),
                        already_exists: false,
                    };
                    let _ = tx.send(Ok(rsp)).await;
                    responded_chunks.insert(chunk_hash);
                }
                continue;
            }

            // 注意：一个 chunk 可能会被分多次（多个 part）上传。
            // 因此这里不能直接用 `has_chunk()` 去校验哈希，否则 partial file 会被误判为 HashMismatch。
            // 策略：
            // - 如果本地文件已存在且长度 == chunk_size（已完整），再进行 `has_chunk()` 校验；校验通过则直接返回 already exists。
            // - 如果长度 < chunk_size，视为 partial，允许继续 append（必要时可按 offset 决定是否清理重传）。
            // - 如果长度 > chunk_size，视为脏数据，删除并报错。
            // - 如果 chunk_size <= 0（未知），退回到 `has_chunk()` 的严格校验逻辑。
            let path = match cache_service().chunk_path_unchecked(&chunk_hash) {
                Ok(p) => p,
                Err(e) => {
                    if !responded_chunks.contains(&chunk_hash) {
                        let rsp = UploadFileChunkRsp {
                            ticket,
                            success: false,
                            chunk_hash: chunk_hash.clone(),
                            message: format!("invalid chunk hash: {e}"),
                            already_exists: false,
                        };
                        let _ = tx.send(Ok(rsp)).await;
                        responded_chunks.insert(chunk_hash);
                    }
                    continue;
                }
            };

            if let Ok(meta) = std::fs::metadata(&path) {
                let current_len = meta.len();

                if chunk_size > 0 {
                    if current_len == chunk_size {
                        // 已完整：严格校验哈希，避免脏数据冒充
                        match cache_service().has_chunk(&chunk_hash) {
                            Ok(true) => {
                                if !responded_chunks.contains(&chunk_hash) {
                                    let rsp = UploadFileChunkRsp {
                                        ticket,
                                        success: true,
                                        chunk_hash: chunk_hash.clone(),
                                        message: "already exists".to_string(),
                                        already_exists: true,
                                    };
                                    let _ = tx.send(Ok(rsp)).await;
                                    responded_chunks.insert(chunk_hash);
                                }
                                continue;
                            }
                            Ok(false) => {}
                            Err(ChunkCacheError::HashMismatch { expected: _, actual: _ }) => {
                                // 已完整但哈希不一致：删除脏数据；若客户端从 offset=0 重新上传则放行，否则返回失败提示重试。
                                let _ = cache_service().remove_chunk(&chunk_hash);
                                if offset != 0 && !responded_chunks.contains(&chunk_hash) {
                                    let rsp = UploadFileChunkRsp {
                                        ticket,
                                        success: false,
                                        chunk_hash: chunk_hash.clone(),
                                        message:
                                            "existing corrupted chunk removed; please retry from offset 0"
                                                .to_string(),
                                        already_exists: false,
                                    };
                                    let _ = tx.send(Ok(rsp)).await;
                                    responded_chunks.insert(chunk_hash);
                                    continue;
                                }
                            }
                            Err(e) => {
                                let _ = cache_service().remove_chunk(&chunk_hash);
                                if !responded_chunks.contains(&chunk_hash) {
                                    let rsp = UploadFileChunkRsp {
                                        ticket,
                                        success: false,
                                        chunk_hash: chunk_hash.clone(),
                                        message: format!("cache check failed: {e}"),
                                        already_exists: false,
                                    };
                                    let _ = tx.send(Ok(rsp)).await;
                                    responded_chunks.insert(chunk_hash);
                                }
                                continue;
                            }
                        }
                    } else if current_len < chunk_size {
                        // partial：允许继续写；如果客户端从 offset=0 开始重传，则清理旧 partial
                        if offset == 0 && current_len > 0 {
                            let _ = cache_service().remove_chunk(&chunk_hash);
                        }
                    } else {
                        // current_len > chunk_size：脏数据
                        let _ = cache_service().remove_chunk(&chunk_hash);
                        if !responded_chunks.contains(&chunk_hash) {
                            let rsp = UploadFileChunkRsp {
                                ticket,
                                success: false,
                                chunk_hash: chunk_hash.clone(),
                                message:
                                    "cache file larger than declared chunk_size; removed corrupted cache"
                                        .to_string(),
                                already_exists: false,
                            };
                            let _ = tx.send(Ok(rsp)).await;
                            responded_chunks.insert(chunk_hash);
                        }
                        continue;
                    }
                } else {
                    // chunk_size 未知：退回到严格校验逻辑（可能会多读文件，但避免语义不确定）
                    match cache_service().has_chunk(&chunk_hash) {
                        Ok(true) => {
                            if !responded_chunks.contains(&chunk_hash) {
                                let rsp = UploadFileChunkRsp {
                                    ticket,
                                    success: true,
                                    chunk_hash: chunk_hash.clone(),
                                    message: "already exists".to_string(),
                                    already_exists: true,
                                };
                                let _ = tx.send(Ok(rsp)).await;
                                responded_chunks.insert(chunk_hash);
                            }
                            continue;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            let _ = cache_service().remove_chunk(&chunk_hash);
                            if !responded_chunks.contains(&chunk_hash) {
                                let rsp = UploadFileChunkRsp {
                                    ticket,
                                    success: false,
                                    chunk_hash: chunk_hash.clone(),
                                    message: format!("cache check failed: {e}"),
                                    already_exists: false,
                                };
                                let _ = tx.send(Ok(rsp)).await;
                                responded_chunks.insert(chunk_hash);
                            }
                            continue;
                        }
                    }
                }
            }

            // 落盘写入本次 chunk part
            let write_result = cache_service().append_chunk_part(&chunk_hash, offset, &content);

            match write_result {
                Ok(()) => {
                    // 只在 chunk 完整写入（末尾）时，给这个 chunk 回一次“最终响应”
                    let wrote_end =
                        chunk_size > 0 && offset.saturating_add(content.len() as u64) == chunk_size;
                    if wrote_end && !responded_chunks.contains(&chunk_hash) {
                        let (success, message) = match cache_service().has_chunk(&chunk_hash) {
                            Ok(true) => (true, "uploaded".to_string()),
                            Ok(false) => (false, "cache missing after write".to_string()),
                            Err(ChunkCacheError::HashMismatch { expected: _, actual: _ }) => {
                                let _ = cache_service().remove_chunk(&chunk_hash);
                                (false, "hash mismatch; removed corrupted cache".to_string())
                            }
                            Err(e) => {
                                let _ = cache_service().remove_chunk(&chunk_hash);
                                (false, format!("verify failed: {e}"))
                            }
                        };

                        let rsp = UploadFileChunkRsp {
                            ticket,
                            success,
                            chunk_hash: chunk_hash.clone(),
                            message,
                            already_exists: false,
                        };
                        let _ = tx.send(Ok(rsp)).await;
                        responded_chunks.insert(chunk_hash);
                    }
                }
                Err(e) => {
                    // offset mismatch / io error 等：对该 chunk 回一次“最终失败响应”
                    let _ = cache_service().remove_chunk(&chunk_hash);
                    if !responded_chunks.contains(&chunk_hash) {
                        let rsp = UploadFileChunkRsp {
                            ticket,
                            success: false,
                            chunk_hash: chunk_hash.clone(),
                            message: format!("cache write failed: {e}"),
                            already_exists: false,
                        };
                        let _ = tx.send(Ok(rsp)).await;
                        responded_chunks.insert(chunk_hash);
                    }
                }
            }
        }
    });

    Ok(Response::new(ReceiverStream::new(rx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::OnceLock;

    use crate::{
        auth::{AuthService, TokenPolicy},
        caching::ChunkCache,
        hive_server::CrvHiveService,
        hive_server::submit::{cache_service, CACHE_SERVICE},
        pb::hive_service_client::HiveServiceClient,
        pb::hive_service_server::HiveServiceServer,
    };
    use crv_core::repository::compute_chunk_hash;
    use tempfile::tempdir;
    use tokio::sync::oneshot;
    use tonic::transport::Server;

    static INIT: OnceLock<()> = OnceLock::new();

    fn hash_to_hex(data: &[u8]) -> String {
        let hash = compute_chunk_hash(data);
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }

    fn ensure_test_cache() {
        INIT.get_or_init(|| {
            let tmp = tempdir().expect("create temp dir for chunk cache");
            // Keep tempdir so it lives for the entire test process; cache root is under it.
            let tmp_path = tmp.keep();
            let cache_root = tmp_path.join("chunks");
            let cache = ChunkCache::new(cache_root).expect("create ChunkCache");
            let _ = CACHE_SERVICE.set(cache);
        });
    }

    async fn start_test_server() -> (std::net::SocketAddr, oneshot::Sender<()>) {
        ensure_test_cache();

        let auth = std::sync::Arc::new(AuthService::new(b"test-secret", TokenPolicy::default()));
        let service = CrvHiveService::new(auth);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server listener");
        let addr = listener.local_addr().expect("listener local_addr");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let incoming = futures::stream::unfold(listener, |listener| async {
            match listener.accept().await {
                Ok((socket, _peer)) => Some((Ok(socket), listener)),
                Err(e) => Some((Err(e), listener)),
            }
        });

        tokio::spawn(async move {
            let shutdown = async move {
                let _ = shutdown_rx.await;
            };
            Server::builder()
                .add_service(HiveServiceServer::new(service))
                .serve_with_incoming_shutdown(incoming, shutdown)
                .await
                .expect("serve test server");
        });

        (addr, shutdown_tx)
    }

    async fn connect_client(addr: std::net::SocketAddr) -> HiveServiceClient<tonic::transport::Channel> {
        let endpoint = format!("http://{addr}");
        // 等 server socket ready（极少数机器上 connect 可能 race）
        for _ in 0..30 {
            if let Ok(client) = HiveServiceClient::connect(endpoint.clone()).await {
                return client;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        HiveServiceClient::connect(endpoint)
            .await
            .expect("connect HiveServiceClient")
    }

    async fn collect_all_responses(
        mut stream: tonic::Streaming<UploadFileChunkRsp>,
    ) -> Vec<UploadFileChunkRsp> {
        let mut out = Vec::new();
        while let Some(msg) = stream.message().await.expect("stream message") {
            out.push(msg);
        }
        out
    }

    async fn test_upload_success_and_already_exists() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        let data = b"hello upload_file_chunk";
        let chunk_hash = hash_to_hex(data);

        // 1) 首次上传：应返回 uploaded
        let req1 = UploadFileChunkReq {
            ticket: "t1".to_string(),
            chunks_amount: 1,
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            chunk_size: data.len() as i64,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        };
        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(vec![req1]))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 1);
        assert!(rsps[0].success, "expected success=true");
        assert_eq!(rsps[0].chunk_hash, chunk_hash);
        assert_eq!(rsps[0].already_exists, false);
        assert_eq!(rsps[0].message, "uploaded");

        // 2) 再次上传同一 chunk：应立即返回 already exists
        let req2 = UploadFileChunkReq {
            ticket: "t1".to_string(),
            chunks_amount: 1,
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            chunk_size: data.len() as i64,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        };
        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(vec![req2]))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 1);
        assert!(rsps[0].success, "expected success=true");
        assert_eq!(rsps[0].chunk_hash, chunk_hash);
        assert_eq!(rsps[0].already_exists, true);
        assert_eq!(rsps[0].message, "already exists");

        let _ = shutdown_tx.send(());
    }

    async fn test_upload_multiple_chunks_in_one_stream() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        let data1 = b"chunk one";
        let data2 = b"chunk two";
        let hash1 = hash_to_hex(data1);
        let hash2 = hash_to_hex(data2);

        let reqs = vec![
            UploadFileChunkReq {
                ticket: "t_multi".to_string(),
                chunks_amount: 2,
                chunk_hash: hash1.clone(),
                offset: 0,
                content: data1.to_vec(),
                chunk_size: data1.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data1.len() as u32,
            },
            UploadFileChunkReq {
                ticket: "t_multi".to_string(),
                chunks_amount: 2,
                chunk_hash: hash2.clone(),
                offset: 0,
                content: data2.to_vec(),
                chunk_size: data2.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: data2.len() as u32,
            },
        ];

        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(reqs))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let mut rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 2);
        rsps.sort_by(|a, b| a.chunk_hash.cmp(&b.chunk_hash));

        assert!(rsps[0].success);
        assert!(rsps[1].success);
        assert_eq!(rsps[0].already_exists, false);
        assert_eq!(rsps[1].already_exists, false);
        assert_eq!(rsps[0].message, "uploaded");
        assert_eq!(rsps[1].message, "uploaded");
        assert!(
            (rsps[0].chunk_hash == hash1 && rsps[1].chunk_hash == hash2)
                || (rsps[0].chunk_hash == hash2 && rsps[1].chunk_hash == hash1)
        );

        let _ = shutdown_tx.send(());
    }

    async fn test_upload_single_chunk_multiple_parts() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        let part1 = b"hello ";
        let part2 = b"world";
        let full: Vec<u8> = [part1.as_ref(), part2.as_ref()].concat();
        let chunk_hash = hash_to_hex(&full);

        let reqs = vec![
            UploadFileChunkReq {
                ticket: "t_parts".to_string(),
                chunks_amount: 1,
                chunk_hash: chunk_hash.clone(),
                offset: 0,
                content: part1.to_vec(),
                chunk_size: full.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: full.len() as u32,
            },
            UploadFileChunkReq {
                ticket: "t_parts".to_string(),
                chunks_amount: 1,
                chunk_hash: chunk_hash.clone(),
                offset: part1.len() as i64,
                content: part2.to_vec(),
                chunk_size: full.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: full.len() as u32,
            },
        ];

        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(reqs))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let rsps = collect_all_responses(rsp_stream).await;
        // 只在写到末尾时返回一次最终响应
        assert_eq!(rsps.len(), 1);
        assert!(rsps[0].success);
        assert_eq!(rsps[0].chunk_hash, chunk_hash);
        assert_eq!(rsps[0].message, "uploaded");

        let _ = shutdown_tx.send(());
    }

    async fn test_upload_two_chunks_interleaved_parts() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        let a1 = b"aaa";
        let a2 = b"bbb";
        let b1 = b"111";
        let b2 = b"222";

        let full_a: Vec<u8> = [a1.as_ref(), a2.as_ref()].concat();
        let full_b: Vec<u8> = [b1.as_ref(), b2.as_ref()].concat();
        let hash_a = hash_to_hex(&full_a);
        let hash_b = hash_to_hex(&full_b);

        // 交错上传两个 chunk 的 part，chunks_amount=2
        let reqs = vec![
            UploadFileChunkReq {
                ticket: "t_interleave".to_string(),
                chunks_amount: 2,
                chunk_hash: hash_a.clone(),
                offset: 0,
                content: a1.to_vec(),
                chunk_size: full_a.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: full_a.len() as u32,
            },
            UploadFileChunkReq {
                ticket: "t_interleave".to_string(),
                chunks_amount: 2,
                chunk_hash: hash_b.clone(),
                offset: 0,
                content: b1.to_vec(),
                chunk_size: full_b.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: full_b.len() as u32,
            },
            UploadFileChunkReq {
                ticket: "t_interleave".to_string(),
                chunks_amount: 2,
                chunk_hash: hash_a.clone(),
                offset: a1.len() as i64,
                content: a2.to_vec(),
                chunk_size: full_a.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: full_a.len() as u32,
            },
            UploadFileChunkReq {
                ticket: "t_interleave".to_string(),
                chunks_amount: 2,
                chunk_hash: hash_b.clone(),
                offset: b1.len() as i64,
                content: b2.to_vec(),
                chunk_size: full_b.len() as i64,
                compression: "none".to_string(),
                uncompressed_size: full_b.len() as u32,
            },
        ];

        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(reqs))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let mut rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 2);
        rsps.sort_by(|a, b| a.chunk_hash.cmp(&b.chunk_hash));

        assert!(rsps[0].success);
        assert!(rsps[1].success);
        assert_eq!(rsps[0].message, "uploaded");
        assert_eq!(rsps[1].message, "uploaded");
        assert!(
            (rsps[0].chunk_hash == hash_a && rsps[1].chunk_hash == hash_b)
                || (rsps[0].chunk_hash == hash_b && rsps[1].chunk_hash == hash_a)
        );

        let _ = shutdown_tx.send(());
    }

    async fn test_rejects_unsupported_compression() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        let data = b"hello";
        let chunk_hash = hash_to_hex(data);

        let req = UploadFileChunkReq {
            ticket: "t2".to_string(),
            chunks_amount: 1,
            chunk_hash: chunk_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            chunk_size: data.len() as i64,
            compression: "lz4".to_string(),
            uncompressed_size: data.len() as u32,
        };
        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(vec![req]))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 1);
        assert!(!rsps[0].success);
        assert_eq!(rsps[0].chunk_hash, chunk_hash);
        assert!(rsps[0].message.contains("unsupported compression"));

        let _ = shutdown_tx.send(());
    }

    async fn test_offset_mismatch_returns_failure() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        // 通过首包 offset != 0 命中 append_chunk_part 的 offset mismatch 分支：
        // - 文件尚不存在 => has_chunk() == Ok(false) 不会提前失败
        // - append_chunk_part() 发现 current_len(=0) != offset(=1) => write failed
        let data = b"offset mismatch";
        let chunk_hash = hash_to_hex(data);
        let reqs = vec![UploadFileChunkReq {
            ticket: "t3".to_string(),
            chunks_amount: 1,
            chunk_hash: chunk_hash.clone(),
            offset: 1,
            content: data.to_vec(),
            chunk_size: data.len() as i64,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        }];

        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(reqs))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 1);
        assert!(!rsps[0].success);
        assert_eq!(rsps[0].chunk_hash, chunk_hash);
        assert!(rsps[0].message.contains("cache write failed"));

        let _ = shutdown_tx.send(());
    }

    async fn test_hash_mismatch_is_removed() {
        cache_service().clear_all().expect("clear cache");

        let (addr, shutdown_tx) = start_test_server().await;
        let mut client = connect_client(addr).await;

        let data = b"actual data";
        let wrong_hash = hash_to_hex(b"some other content");

        let req = UploadFileChunkReq {
            ticket: "t4".to_string(),
            chunks_amount: 1,
            chunk_hash: wrong_hash.clone(),
            offset: 0,
            content: data.to_vec(),
            chunk_size: data.len() as i64,
            compression: "none".to_string(),
            uncompressed_size: data.len() as u32,
        };
        let rsp_stream = client
            .upload_file_chunk(tokio_stream::iter(vec![req]))
            .await
            .expect("upload_file_chunk call")
            .into_inner();
        let rsps = collect_all_responses(rsp_stream).await;
        assert_eq!(rsps.len(), 1);
        assert!(!rsps[0].success);
        assert_eq!(rsps[0].chunk_hash, wrong_hash);
        assert_eq!(rsps[0].message, "hash mismatch; removed corrupted cache");

        // 确认已删除（不再存在，也不会再触发 HashMismatch）
        assert_eq!(
            cache_service().has_chunk(&wrong_hash).expect("has_chunk"),
            false
        );

        let _ = shutdown_tx.send(());
    }

    /// 统一 harness：共享一个 tokio runtime，避免多 runtime 叠加带来的不稳定。
    #[test]
    fn upload_file_chunk_tests_harness() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .expect("build tokio runtime for upload_file_chunk tests");

        rt.block_on(async {
            test_upload_success_and_already_exists().await;
            test_upload_multiple_chunks_in_one_stream().await;
            test_upload_single_chunk_multiple_parts().await;
            test_upload_two_chunks_interleaved_parts().await;
            test_rejects_unsupported_compression().await;
            test_offset_mismatch_returns_failure().await;
            test_hash_mismatch_is_removed().await;
        });
    }
}