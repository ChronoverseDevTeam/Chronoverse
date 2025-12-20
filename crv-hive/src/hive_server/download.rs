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
    let total_len = bytes.len();
    let total_len_u32 = u32::try_from(total_len).unwrap_or(u32::MAX);

    if total_len == 0 {
        return vec![DownloadFileChunkResp {
            chunk_hash: chunk_hash.to_string(),
            offset: 0,
            content: Vec::new(),
            eof: true,
            compression: "none".to_string(),
            uncompressed_size: 0,
        }];
    }

    let mut out = Vec::new();
    let mut offset: usize = 0;
    while offset < total_len {
        let end = (offset + packet_size).min(total_len);
        let eof = end == total_len;
        out.push(DownloadFileChunkResp {
            chunk_hash: chunk_hash.to_string(),
            offset: offset as i64,
            content: bytes[offset..end].to_vec(),
            eof,
            compression: "none".to_string(),
            uncompressed_size: total_len_u32,
        });
        offset = end;
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use crv_core::repository::{Compression, blake3_hash_to_hex};
    use tempfile::tempdir;

    #[test]
    fn normalize_chunk_hashes_trims_lowercases_and_filters_empty() {
        let input = vec![
            "  ABC  ".to_string(),
            "".to_string(),
            "   ".to_string(),
            "DeF".to_string(),
        ];
        let out = normalize_chunk_hashes(input);
        assert_eq!(out, vec!["abc".to_string(), "def".to_string()]);
    }

    #[test]
    fn normalize_packet_size_defaults_and_clamps() {
        assert_eq!(normalize_packet_size(0), 256 * 1024);
        assert_eq!(normalize_packet_size(-1), 256 * 1024);
        assert_eq!(normalize_packet_size(1), 1);
        assert_eq!(normalize_packet_size(i64::MAX), 4 * 1024 * 1024);
    }

    #[test]
    fn chunk_bytes_to_responses_empty_chunk_emits_single_eof_packet() {
        let out = chunk_bytes_to_responses("h", &[], 16);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].chunk_hash, "h");
        assert_eq!(out[0].offset, 0);
        assert!(out[0].eof);
        assert_eq!(out[0].content.len(), 0);
        assert_eq!(out[0].compression, "none");
        assert_eq!(out[0].uncompressed_size, 0);
    }

    #[test]
    fn chunk_bytes_to_responses_splits_and_sets_offsets_and_eof() {
        let bytes = b"abcdef";
        let out = chunk_bytes_to_responses("hash", bytes, 2);
        assert_eq!(out.len(), 3);

        assert_eq!(out[0].offset, 0);
        assert_eq!(out[0].content, b"ab");
        assert!(!out[0].eof);

        assert_eq!(out[1].offset, 2);
        assert_eq!(out[1].content, b"cd");
        assert!(!out[1].eof);

        assert_eq!(out[2].offset, 4);
        assert_eq!(out[2].content, b"ef");
        assert!(out[2].eof);

        for p in &out {
            assert_eq!(p.chunk_hash, "hash");
            assert_eq!(p.compression, "none");
            assert_eq!(p.uncompressed_size, bytes.len() as u32);
        }
    }

    #[test]
    fn chunk_bytes_to_responses_packet_size_larger_than_chunk_is_single_packet() {
        let bytes = b"abc";
        let out = chunk_bytes_to_responses("hash", bytes, 1024);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].offset, 0);
        assert_eq!(out[0].content, b"abc");
        assert!(out[0].eof);
    }

    #[test]
    fn multi_chunk_order_can_be_preserved_by_concatenation() {
        let a = chunk_bytes_to_responses("a", b"12", 1);
        let b = chunk_bytes_to_responses("b", b"XYZ", 2);
        let all: Vec<_> = a.into_iter().chain(b.into_iter()).collect();
        assert_eq!(all[0].chunk_hash, "a");
        assert_eq!(all[0].content, b"1");
        assert_eq!(all[1].chunk_hash, "a");
        assert_eq!(all[1].content, b"2");
        assert_eq!(all[2].chunk_hash, "b");
        assert_eq!(all[2].content, b"XY");
        assert_eq!(all[3].chunk_hash, "b");
        assert_eq!(all[3].content, b"Z");
        assert!(all[3].eof);
    }

    #[test]
    fn download_from_repo_reads_real_repository_and_splits_packets() {
        let tmp = tempdir().unwrap();
        let repo = Repository::new(tmp.path()).expect("create repo");

        let record1 = repo
            .write_chunk(b"hello", Compression::None)
            .expect("write chunk1");
        let record2 = repo
            .write_chunk(b"world!!!", Compression::None)
            .expect("write chunk2");

        let h1 = blake3_hash_to_hex(&record1.hash);
        let h2 = blake3_hash_to_hex(&record2.hash);

        let chunk_hashes = vec![h1.clone(), h2.clone()];
        let out = download_from_repo(&repo, &chunk_hashes, 3).expect("download_from_repo");

        // chunk1: "hello" -> 2 packets ("hel", "lo")
        assert_eq!(out[0].chunk_hash, h1);
        assert_eq!(out[0].offset, 0);
        assert_eq!(out[0].content, b"hel");
        assert!(!out[0].eof);

        assert_eq!(out[1].chunk_hash, h1);
        assert_eq!(out[1].offset, 3);
        assert_eq!(out[1].content, b"lo");
        assert!(out[1].eof);

        // chunk2: "world!!!" -> 3 packets ("wor","ld!","!!")
        assert_eq!(out[2].chunk_hash, h2);
        assert_eq!(out[2].offset, 0);
        assert_eq!(out[2].content, b"wor");
        assert!(!out[2].eof);

        assert_eq!(out[3].chunk_hash, h2);
        assert_eq!(out[3].offset, 3);
        assert_eq!(out[3].content, b"ld!");
        assert!(!out[3].eof);

        assert_eq!(out[4].chunk_hash, h2);
        assert_eq!(out[4].offset, 6);
        assert_eq!(out[4].content, b"!!");
        assert!(out[4].eof);
    }

    #[test]
    fn download_from_repo_returns_not_found_for_missing_chunk() {
        let tmp = tempdir().unwrap();
        let repo = Repository::new(tmp.path()).expect("create repo");

        // 随便给一个合法的 64 位 hex作为不存在的 chunk
        let missing = "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let err = download_from_repo(&repo, &[missing], 16).unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[test]
    fn download_from_repo_returns_invalid_argument_for_bad_hash() {
        let tmp = tempdir().unwrap();
        let repo = Repository::new(tmp.path()).expect("create repo");

        let bad = "not-a-hash".to_string();
        let err = download_from_repo(&repo, &[bad], 16).unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }
}


