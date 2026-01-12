use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::context::SessionContext;
use crate::daemon_server::db::active_file;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crate::hive_pb::hive_service_client::HiveServiceClient;
use crate::hive_pb::{
    self, CheckChunksReq, FileChunks, FileToLock, LaunchSubmitReq, UploadFileChunkReq,
};
use crate::pb::{SubmitProgress, SubmitReq};
use crv_core::path::basic::{DepotPath, LocalPath, PathError, WorkspaceDir, WorkspacePath};
use crv_core::path::engine::PathEngine;
use crv_core::repository::compute_chunk_hash;
use futures::stream::{self, StreamExt};
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use walkdir::WalkDir;

pub type SubmitProgressStream =
    Pin<Box<dyn Stream<Item = Result<SubmitProgress, Status>> + Send + Sync + 'static>>;

struct FileToSubmit {
    local_path: LocalPath,
    workspace_path: WorkspacePath,
    depot_path: DepotPath,
    action: active_file::Action,
    current_revision: Option<String>,
}

const FRAME_SIZE: usize = 64 * 1024; // 64KB，单个报文中的数据大小
const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB，内存中的处理窗口，也是一个 chunk 的大小

pub async fn handle(
    state: AppState,
    req: Request<SubmitReq>,
) -> AppResult<Response<SubmitProgressStream>> {
    let _ctx = SessionContext::from_req(&req)?;
    let request_body = req.get_ref();
    let workspace_meta = state
        .db
        .get_confirmed_workspace_meta(&request_body.workspace_name)?
        .ok_or(AppError::Raw(Status::not_found(format!(
            "Workspace {} not found.",
            request_body.workspace_name
        ))))?;

    // step 1. 将用户指定的路径转化为实际的本地绝对路径，并获得待提交文件
    // 用户指定的路径可能有两种：本地绝对路径（文件或目录）、WorkspaceDir 或者 WorkspacePath
    // step 1.1. 找出所有 WorkspacePath，将其转化为本地绝对路径
    let local_file_from_workspace_path = request_body
        .paths
        .iter()
        .filter(|x| x.starts_with("//") && !x.ends_with("/"))
        .map(|x| WorkspacePath::parse(x))
        .collect::<Result<Vec<WorkspacePath>, PathError>>()
        .map_err(|e| {
            AppError::Raw(Status::invalid_argument(format!(
                "Can't parse workspace path: {e}"
            )))
        })? // 1. 将 workspace path 解析出来
        .iter()
        .map(|x| {
            if x.workspace_name == request_body.workspace_name {
                Ok(x.clone())
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Workspace path {} is not under workspace {}.",
                    x.to_string(),
                    request_body.workspace_name
                ))))
            }
        }) // 2. 检查是否是当前工作区的 workspace path
        .collect::<AppResult<Vec<WorkspacePath>>>()?
        .iter()
        .map(|x| {
            let local_path = x
                .into_local_path(&workspace_meta.config.root_dir)
                .to_local_path_string();
            if Path::new(&local_path).exists() {
                Ok(local_path)
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not exist.",
                    local_path
                ))))
            }
        }) // 3. 转化为本地绝对路径，检查是否存在
        .collect::<AppResult<Vec<String>>>()?;

    let local_dir_from_workspace_dir = request_body
        .paths
        .iter()
        .filter(|x| x.starts_with("//") && x.ends_with("/"))
        .map(|x| WorkspaceDir::parse(x))
        .collect::<Result<Vec<WorkspaceDir>, PathError>>()
        .map_err(|e| {
            AppError::Raw(Status::invalid_argument(format!(
                "Can't parse workspace dir: {e}"
            )))
        })? // 1. 将 workspace dir 解析出来
        .iter()
        .map(|x| {
            if x.workspace_name == request_body.workspace_name {
                Ok(x.clone())
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Workspace path {} is not under workspace {}.",
                    x.to_string(),
                    request_body.workspace_name
                ))))
            }
        }) // 2. 检查是否是当前工作区的 workspace dir
        .collect::<AppResult<Vec<WorkspaceDir>>>()?
        .iter()
        .map(|x| {
            let local_path = x
                .into_local_dir(&workspace_meta.config.root_dir)
                .to_local_path_string();
            if Path::new(&local_path).exists() {
                Ok(local_path)
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not exist.",
                    local_path
                ))))
            }
        }) // 3. 转化为本地绝对路径，检查是否存在
        .collect::<AppResult<Vec<String>>>()?;

    let local_paths = request_body
        .paths
        .iter()
        .filter(|x| !x.starts_with("//"))
        .map(|x| {
            if Path::new(x).exists() {
                Ok(x.clone())
            } else {
                Err(AppError::Raw(Status::invalid_argument(format!(
                    "Path {} does not exist.",
                    x
                ))))
            }
        }) //检查是否存在
        .collect::<AppResult<Vec<String>>>()?
        .iter()
        .chain(local_dir_from_workspace_dir.iter())
        .chain(local_file_from_workspace_path.iter()) // 合起来！
        .map(|x| x.clone())
        .collect::<Vec<String>>();

    let local_files = local_paths
        .iter()
        .flat_map(|path| {
            WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok()) // 过滤掉读取失败的条目
                .filter(|e| e.file_type().is_file()) // 只保留文件
                .map(|e| e.path().to_string_lossy().into_owned())
        })
        .collect::<Vec<String>>();

    let path_engine = PathEngine::new(workspace_meta.config.clone());

    let mut files_to_submit: Vec<FileToSubmit> = Vec::new();

    for file in &local_files {
        // 进行路径转化与过滤，经过这一操作后，local_path 已经是该工作区下被映射了的文件
        let local_path = LocalPath::parse(&file).unwrap();
        let workspace_path = path_engine.local_path_to_workspace_path(&local_path);
        let depot_path = path_engine.mapping_local_path(&local_path);
        if workspace_path.is_none() || depot_path.is_none() {
            continue;
        }
        let workspace_path = workspace_path.unwrap();
        let depot_path = depot_path.unwrap();
        // 判断文件是否被 checkout
        let file_action = state.db.get_active_file_action(&workspace_path)?;
        if file_action.is_none() {
            continue;
        }
        let file_action = file_action.unwrap();
        // 获得文件当前 revision
        let file_revision = if file_action == active_file::Action::Add {
            None
        } else {
            let file_meta = state.db.get_file_meta(&workspace_path)?.unwrap();
            Some(file_meta.latest_revision)
        };
        files_to_submit.push(FileToSubmit {
            local_path,
            workspace_path,
            depot_path,
            action: file_action,
            current_revision: file_revision,
        });
    }

    let runtime_config = RuntimeConfig::from_req(&req)?;
    let channel = state
        .hive_channel
        .get_channel(&runtime_config.remote_addr.value)?;

    let mut hive_client = HiveServiceClient::new(channel.clone());

    // step 2. TryLockFiles：锁定所有待提交文件
    let mut files_to_lock = Vec::new();
    for file in &files_to_submit {
        files_to_lock.push(FileToLock {
            path: file.depot_path.to_string(),
            expected_file_revision: file.current_revision.clone().unwrap_or(String::new()),
            expected_file_not_exist: file.action == active_file::Action::Add,
        });
    }

    let try_lock_req = LaunchSubmitReq {
        files: files_to_lock,
    };

    let try_lock_file_response = hive_client.launch_submit(try_lock_req).await?.into_inner();
    if !try_lock_file_response.success {
        // todo 这里是乐观锁，讲道理应该多试几次
        return Err(AppError::Internal(format!(
            "Can't lock files: {:?}!",
            try_lock_file_response.file_unable_to_lock
        )));
    }

    let ticket = try_lock_file_response.ticket;

    // step 3. 初始化通信管道
    // upload_tx: 将 Chunk 发送给网络上传任务
    let (upload_tx, upload_rx) = mpsc::channel::<UploadFileChunkReq>(256);
    // response_tx: 将进度发送给客户端
    let (response_tx, response_rx) = mpsc::channel(128);

    // step 4. 启动 chunk 上传消费者
    let mut hive_client_upload = HiveServiceClient::new(channel.clone());
    let upload_task = tokio::spawn(async move {
        let request_stream = ReceiverStream::new(upload_rx);
        hive_client_upload.upload_file_chunk(request_stream).await
    });

    // step 5. 启动进度回报消费者
    // tokio::spawn(track_progress(response_tx.clone(), progress_watch_rx));

    // step 6. 构造 chunk 上传生产者流
    // 用于收集所有已切块文件的元数据
    let submitted_files = Arc::new(Mutex::new(Vec::<FileChunks>::new()));
    let upload_tx_clone = upload_tx.clone();
    let response_tx_clone = response_tx.clone();
    let ticket_clone = ticket.clone();
    let channel_clone = channel.clone();
    let submitted_files_clone = submitted_files.clone();
    let futs = stream::iter(files_to_submit).map(move |file_info| {
        let up_tx = upload_tx_clone.clone();
        let rsp_tx = response_tx_clone.clone();
        let tk_clone = ticket_clone.clone();
        let chan = channel_clone.clone();
        let s_files = submitted_files_clone.clone();

        async move {
            // 如果是删除行为，则直接回报即可
            if file_info.action == active_file::Action::Delete {
                rsp_tx
                    .send(Ok(SubmitProgress {
                        path: file_info.local_path.to_local_path_string(),
                        bytes_completed_so_far: 0i64,
                    }))
                    .await
                    .map_err(|_| AppError::Internal("Response channel closed".into()))?;

                let submit_file = FileChunks {
                    path: file_info.depot_path.to_string(), // 使用服务器路径
                    binary_id: vec![],                      // 块 Hash 列表
                };

                s_files.lock().unwrap().push(submit_file);
                return Ok::<(), AppError>(());
            }

            let path_str = file_info.local_path.to_local_path_string();
            let mut file = File::open(&path_str)
                .await
                .map_err(|e| AppError::Internal(format!("Open error: {e}")))?;

            let mut chunk_buffer = vec![0u8; CHUNK_SIZE];
            let mut hive_client = HiveServiceClient::new(chan);
            let mut chunk_hashes = vec![]; // 收集当前文件的所有块 hash
            let mut total_size = 0i64;

            loop {
                // 读取一个 chunk
                let n = file
                    .read(&mut chunk_buffer)
                    .await
                    .map_err(|e| AppError::Internal(format!("Read error: {e}")))?;

                if n == 0 {
                    break;
                }
                let data = &chunk_buffer[..n];

                let frames: Vec<&[u8]> = data.chunks(FRAME_SIZE).collect();
                // 记录 Hash
                let chunk_hash = hex::encode(compute_chunk_hash(data));
                chunk_hashes.push(chunk_hash.clone());

                // 秒传逻辑：Check Chunks
                let check_res = hive_client
                    .check_chunks(CheckChunksReq {
                        chunk_hashes: vec![chunk_hash.clone()],
                    })
                    .await?
                    .into_inner();

                // 如果这个 chunk 已经传输完毕，则跳过
                if check_res.missing_chunk_hashes.is_empty() {
                    // 更新进度
                    total_size += n as i64;
                    rsp_tx
                        .send(Ok(SubmitProgress {
                            path: file_info.local_path.to_local_path_string(),
                            bytes_completed_so_far: total_size,
                        }))
                        .await
                        .map_err(|_| AppError::Internal("Response channel closed".into()))?;
                    continue;
                }

                // 4. 遍历切片并上传
                let mut offset = 0i64;
                for (i, frame_data) in frames.iter().enumerate() {
                    up_tx
                        .send(UploadFileChunkReq {
                            chunk_hash: chunk_hash.clone(),
                            offset,
                            content: frame_data.to_vec(), // 这里必须这就得 clone 数据发送了
                            compression: "none".to_string(),
                            uncompressed_size: frame_data.len() as u32,
                            ticket: tk_clone.clone(),
                            chunk_size: n as i64,
                        })
                        .await
                        .map_err(|_| AppError::Internal("Upload channel closed".into()))?;

                    // 更新进度
                    offset += frame_data.len() as i64;
                    total_size += frame_data.len() as i64;
                    rsp_tx
                        .send(Ok(SubmitProgress {
                            path: file_info.local_path.to_local_path_string(),
                            bytes_completed_so_far: total_size,
                        }))
                        .await
                        .map_err(|_| AppError::Internal("Response channel closed".into()))?;
                }
            }

            // --- 文件切块完成，收集文件 FileChunks 信息 ---
            let submit_file = FileChunks {
                path: file_info.depot_path.to_string(), // 使用服务器路径
                binary_id: chunk_hashes,                // 块 Hash 列表
            };

            s_files.lock().unwrap().push(submit_file);
            Ok::<(), AppError>(())
        }
    });

    // step 7. 驱动生产者开始工作
    let ticket_for_commit = ticket.clone();
    let description = request_body.description.clone();
    tokio::spawn(async move {
        // A. 等待所有文件切块并发送到管道
        let results: Vec<_> = futs.buffer_unordered(8).collect().await;

        // B. 检查切块过程是否有本地错误（如读取文件失败）
        for res in results {
            if let Err(e) = res {
                let _ = response_tx
                    .send(Err(Status::internal(format!("Local process error: {e}"))))
                    .await;
                return; // 发生错误，提前终止，不执行 Commit
            }
        }

        // C. 关闭管道，通知 upload_task 数据已全部发送
        drop(upload_tx);

        // D. 等待网络上传任务彻底完成
        match upload_task.await {
            Ok(Ok(_)) => {
                // 上传成功，准备 Submit
                let final_files = submitted_files.lock().unwrap().clone();
                let submit_req = hive_pb::SubmitReq {
                    description,
                    files: final_files,
                    ticket: ticket_for_commit,
                };

                let mut client = HiveServiceClient::new(channel.clone());
                match client.submit(submit_req).await {
                    Ok(_) => {
                        // 这里的逻辑可以根据业务调整：
                        // 发送一个特殊的进度包，或者直接关闭流表示成功
                        println!("Commit success!");
                    }
                    Err(status) => {
                        let _ = response_tx.send(Err(status)).await;
                    }
                }
            }
            Ok(Err(status)) => {
                // 服务端返回的上传错误（如校验失败）
                let _ = response_tx.send(Err(status)).await;
            }
            Err(e) => {
                // Task panic
                let _ = response_tx.send(Err(Status::internal(e.to_string()))).await;
            }
        }
    });

    // step 8. 返回 Response Stream
    let output_stream = ReceiverStream::new(response_rx);
    Ok(Response::new(
        Box::pin(output_stream) as SubmitProgressStream
    ))
}
