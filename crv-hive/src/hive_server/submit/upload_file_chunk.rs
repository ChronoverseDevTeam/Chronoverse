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

            // 如果 chunk 已完整存在，则直接告知客户端，无需重复写入。
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
                    // 文件存在但哈希不一致等情况：返回错误，并尝试清理脏数据（若可）。
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