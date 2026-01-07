use std::vec;

use crate::auth;
use crate::hive_server::repository_manager;
use crate::pb::{DownloadFileChunkReq, DownloadFileChunkResp};
use crv_core::repository::{Repository, RepositoryError, blake3_hex_to_hash};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub type DownloadFileChunkStream = ReceiverStream<Result<DownloadFileChunkResp, Status>>;

fn repo_error_to_status(e: RepositoryError) -> Status {
    match e {
        RepositoryError::ChunkNotFound { .. } => Status::not_found("chunk not found"),
        other => Status::internal(other.to_string()),
    }
}

fn normalize_chunk_hashes(chunk_hashes: Vec<String>) -> Vec<String> {
    chunk_hashes
        .into_iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn normalize_packet_size(packet_size: i64) -> usize {
    // packet_size <= 0 时使用默认值；并做上限保护，避免单包过大占用内存。
    let packet_size = if packet_size <= 0 {
        256 * 1024usize
    } else {
        packet_size as usize
    };
    packet_size.clamp(1, 4 * 1024 * 1024)
}

fn chunk_bytes_to_responses(
    chunk_hash: &str,
    bytes: &[u8],
    packet_size: usize,
) -> Vec<DownloadFileChunkResp> {
    vec![]
}

fn download_from_repo(
    repo: &Repository,
    chunk_hashes: &[String],
    packet_size: usize,
) -> Result<Vec<DownloadFileChunkResp>, Status> {
    let mut out = Vec::new();

    for chunk_hash in chunk_hashes {
        let hash_bytes = blake3_hex_to_hash(chunk_hash).ok_or_else(|| {
            Status::invalid_argument(format!("invalid chunk hash: {chunk_hash}"))
        })?;

        let bytes = repo
            .read_chunk(&hash_bytes)
            .map_err(repo_error_to_status)?;

        out.extend(chunk_bytes_to_responses(chunk_hash, &bytes, packet_size));
    }

    Ok(out)
}

pub async fn handle_download_file_chunk(
    request: Request<DownloadFileChunkReq>,
) -> Result<Response<DownloadFileChunkStream>, Status> {
    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let _user = auth::require_user(&request)?;

    let req = request.into_inner();
    let chunk_hashes = normalize_chunk_hashes(req.chunk_hashes);

    if chunk_hashes.is_empty() {
        return Err(Status::invalid_argument("chunk_hashes is required"));
    }

    let packet_size = normalize_packet_size(req.packet_size);

    let (tx, rx) = mpsc::channel::<Result<DownloadFileChunkResp, Status>>(32);

    tokio::spawn(async move {
        let repo = match repository_manager() {
            Ok(r) => r,
            Err(status) => {
                let _ = tx.send(Err(status)).await;
                return;
            }
        };

        let resps = match download_from_repo(repo, &chunk_hashes, packet_size) {
            Ok(r) => r,
            Err(status) => {
                let _ = tx.send(Err(status)).await;
                return;
            }
        };

        for resp in resps {
            if tx.send(Ok(resp)).await.is_err() {
                // 客户端已断开连接
                return;
            }
        }
    });

    Ok(Response::new(ReceiverStream::new(rx)))
}
