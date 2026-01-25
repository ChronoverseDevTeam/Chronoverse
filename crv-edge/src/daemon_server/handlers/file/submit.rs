use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::context::SessionContext;
use crate::daemon_server::db::active_file;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{expand_paths_to_files, normalize_paths};
use crate::daemon_server::job::{
    JobEvent, JobRetentionPolicy, JobStatus, MessageStoragePolicy, WorkerProtocol,
};
use crate::daemon_server::state::AppState;
use crate::hive_pb::hive_service_client::HiveServiceClient;
use crate::hive_pb::{CheckChunksReq, FileChunks, FileToLock, LaunchSubmitReq, UploadFileChunkReq};
use crate::pb::{SubmitProgress, SubmitReq};
use crv_core::path::basic::{DepotPath, LocalPath, WorkspacePath};
use crv_core::path::engine::PathEngine;
use crv_core::repository::compute_chunk_hash;
use prost::Message;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use tokio::sync::Mutex;
use tokio::{fs::File, io::AsyncReadExt};
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::{Stream, StreamExt};
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

pub type SubmitProgressStream =
    Pin<Box<dyn Stream<Item = Result<SubmitProgress, Status>> + Send + Sync + 'static>>;

struct JobCancelOnDropStream {
    stream: SubmitProgressStream,
    job: Weak<crate::daemon_server::job::Job>,
}

impl Stream for JobCancelOnDropStream {
    type Item = Result<SubmitProgress, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next(cx)
    }
}

impl Drop for JobCancelOnDropStream {
    fn drop(&mut self) {
        if let Some(job) = self.job.upgrade() {
            job.cancel();
        }
    }
}

struct FileToSubmit {
    local_path: LocalPath,
    workspace_path: WorkspacePath,
    depot_path: DepotPath,
    action: active_file::Action,
    current_revision: Option<String>,
}

const FRAME_SIZE: usize = 64 * 1024; // 64KB，单个报文中的数据大小
const CHUNK_SIZE: usize = 4 * 1024 * 1024; // 4MB，内存中的处理窗口，也是一个 chunk 的大小
const WORKER_COUNT: i32 = 8;
const MAX_RETRY: i32 = 3; // 单个 chunk 的最大重试次数

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

    // 规范化路径
    let local_paths = normalize_paths(
        &request_body.paths,
        &request_body.workspace_name,
        &workspace_meta.config,
    )?;

    let local_files = expand_paths_to_files(&local_paths);

    let path_engine = PathEngine::new(workspace_meta.config.clone(), &request_body.workspace_name);

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

    // step 3. 创建 Job
    let job = state.job_manager.create_job(
        None,
        MessageStoragePolicy::None,
        WorkerProtocol::And,
        JobRetentionPolicy::Immediate,
    );

    let rx = job.tx.subscribe();
    let description = request_body.description.clone();
    let files_to_submit = Arc::new(Mutex::new(files_to_submit));
    let file_chunks = Arc::new(Mutex::new(vec![]));

    let (marker_tx, marker_rx) = tokio::sync::mpsc::channel::<()>(1);

    for i in 0..WORKER_COUNT {
        let files = files_to_submit.clone();
        let file_chunks = file_chunks.clone();
        let channel_clone = channel.clone();
        let job_clone = job.clone();
        let ticket_clone = ticket.clone();
        let marker_clone = marker_tx.clone();
        job.add_worker(async move {
            upload_task(
                files,
                file_chunks,
                channel_clone,
                ticket_clone,
                job_clone,
                marker_clone,
                i,
            )
            .await
        });
    }

    job.add_worker(async move {
        submit_task(
            state.clone(),
            ticket,
            description,
            file_chunks,
            files_to_submit,
            channel,
            marker_rx,
        )
        .await
    });

    drop(marker_tx);

    job.clone().start();

    // step 4. 返回 Response Stream
    let output_stream = BroadcastStream::new(rx).filter_map(move |res| match res {
        Ok(event) => match event {
            JobEvent::Payload(any) => SubmitProgress::decode(&any.value[..]).ok().map(Ok),
            JobEvent::Error(e) => Some(Err(Status::internal(e))),
            JobEvent::StatusChange(JobStatus::Failed(e)) => Some(Err(Status::internal(e))),
            JobEvent::StatusChange(JobStatus::Cancelled) => {
                Some(Err(Status::cancelled("Submit cancelled")))
            }
            _ => None,
        },
        Err(_) => Some(Err(Status::internal("Stream lagged"))),
    });

    let wrapped_stream = JobCancelOnDropStream {
        stream: Box::pin(output_stream),
        job: Arc::downgrade(&job),
    };

    Ok(Response::new(
        Box::pin(wrapped_stream) as SubmitProgressStream
    ))
}

async fn submit_task(
    state: AppState,
    ticket: String,
    description: String,
    file_chunks: Arc<Mutex<Vec<FileChunks>>>,
    files_to_submit: Arc<Mutex<Vec<FileToSubmit>>>,
    channel: Channel,
    mut marker: tokio::sync::mpsc::Receiver<()>,
) -> Result<(), String> {
    while let Some(_) = marker.recv().await {}
    let mut hive_client = HiveServiceClient::new(channel.clone());
    let submit_request = crate::hive_pb::SubmitReq {
        ticket,
        description,
        files: file_chunks.lock().await.clone(),
    };
    let submit_response = hive_client
        .submit(submit_request)
        .await
        .map_err(|x| format!("{x}"))?
        .into_inner();

    if !submit_response.success {
        return Err(format!(
            "SubmitReq failed with error: {}",
            submit_response.message
        ));
    }

    // 更新数据库
    for file in files_to_submit.lock().await.iter() {
        // 没有产生新版本说明提交失败了
        if !submit_response
            .latest_revision
            .contains_key(&file.depot_path.to_string())
        {
            continue;
        }
        let latest_revision = submit_response
            .latest_revision
            .get(&file.depot_path.to_string())
            .unwrap()
            .revision_id
            .clone();
        state
            .db
            .submit_file(file.workspace_path.clone(), latest_revision)
            .map_err(|x| format!("{x}"))?;
    }

    // todo 这里可以回报一个最终的提交结果给请求方
    Ok(())
}

async fn upload_task(
    files: Arc<Mutex<Vec<FileToSubmit>>>,
    file_chunks: Arc<Mutex<Vec<FileChunks>>>,
    channel: Channel,
    ticket: String,
    job: Arc<crate::daemon_server::job::Job>,
    marker: tokio::sync::mpsc::Sender<()>,
    _worker_id: i32,
) -> Result<(), String> {
    loop {
        if let Some(file_info) = files.lock().await.pop() {
            // 如果是删除行为，则直接回报即可
            if file_info.action == active_file::Action::Delete {
                job.report_payload(SubmitProgress {
                    path: file_info.local_path.to_local_path_string(),
                    bytes_completed_so_far: 0i64,
                    size: 0i64,
                    info: String::new(),
                    warning: String::new(),
                });
                let submit_file = FileChunks {
                    path: file_info.depot_path.to_string(), // 使用服务器路径
                    binary_id: vec![],                      // 块 Hash 列表
                };
                file_chunks.lock().await.push(submit_file);
                continue;
            }
            let path_str = file_info.local_path.to_local_path_string();
            let mut file = File::open(&path_str)
                .await
                .map_err(|e| format!("Open error: {e}"))?;

            let mut hive_client = HiveServiceClient::new(channel.clone());
            let mut chunk_hashes = vec![]; // 收集当前文件的所有块 hash
            let mut total_size = 0i64; // 当前已经传输的总大小
            let file_size = file
                .metadata()
                .await
                .map_err(|x| format!("{x}"))?
                .len() as i64;
            let mut success = false;

            loop {
                // 读取一个 chunk
                let mut chunk_buffer = vec![0u8; CHUNK_SIZE];
                let n = file
                    .read(&mut chunk_buffer)
                    .await
                    .map_err(|e| format!("Read error: {e}"))?;
                // 更新进度
                total_size += n as i64;

                if n == 0 {
                    break;
                }
                // 记录 Hash
                let chunk_hash = hex::encode(compute_chunk_hash(&chunk_buffer[..n]));
                chunk_hashes.push(chunk_hash.clone());

                // 秒传逻辑：Check Chunks
                let check_res = hive_client
                    .check_chunks(CheckChunksReq {
                        chunk_hashes: vec![chunk_hash.clone()],
                    })
                    .await
                    .map_err(|x| format!("{x}"))?
                    .into_inner();

                // 如果这个 chunk 已经传输完毕，则跳过
                if check_res.missing_chunk_hashes.is_empty() {
                    job.report_payload(SubmitProgress {
                        path: file_info.local_path.to_local_path_string(),
                        bytes_completed_so_far: total_size,
                        size: file_size,
                        info: format!("Chunk already exists on hive."),
                        warning: String::new(),
                    });
                    continue;
                }

                // 4. 遍历切片并上传
                success = false;
                let chunk = Arc::new(chunk_buffer);
                for retry in 0..MAX_RETRY {
                    let (tx, rx) = tokio::sync::mpsc::channel(10);
                    let chunk_clone = chunk.clone();
                    let chunk_hash_clone = chunk_hash.clone();
                    let ticket_clone = ticket.clone();
                    let task = tokio::spawn(async move {
                        let mut offset = 0i64;
                        let frames: Vec<&[u8]> = chunk_clone.chunks(FRAME_SIZE).collect();
                        for (i, frame_data) in frames.iter().enumerate() {
                            if tx
                                .send(UploadFileChunkReq {
                                    chunk_hash: chunk_hash_clone.clone(),
                                    offset,
                                    content: frame_data.to_vec(), // 这里必须这就得 clone 数据发送了
                                    compression: "none".to_string(),
                                    uncompressed_size: frame_data.len() as u32,
                                    ticket: ticket_clone.clone(),
                                    chunk_size: n as i64,
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }

                            // 更新进度
                            offset += frame_data.len() as i64;
                        }
                    });

                    let upload_rsp = hive_client
                        .upload_file_chunk(ReceiverStream::new(rx))
                        .await
                        .map_err(|x| format!("{x}"))?;
                    let mut upload_rsp_stream = upload_rsp.into_inner();
                    let mut completed = true;
                    while let Some(rsp) = upload_rsp_stream
                        .message()
                        .await
                        .map_err(|x| format!("{x}"))?
                    {
                        if rsp.success {
                            job.report_payload(SubmitProgress {
                                path: file_info.local_path.to_local_path_string(),
                                bytes_completed_so_far: total_size,
                                size: file_size,
                                info: format!("Upload chunk success."),
                                warning: String::new(),
                            });
                        } else {
                            job.report_payload(SubmitProgress {
                                path: file_info.local_path.to_local_path_string(),
                                bytes_completed_so_far: total_size,
                                size: file_size,
                                info: format!("retry [{}/{MAX_RETRY}]", retry + 1),
                                warning: rsp.message,
                            });
                            completed = false;
                            break;
                        }
                    }
                    // 等待 chunk 上传任务完全结束
                    task.await.unwrap();
                    if !completed {
                        continue;
                    }
                    success = true;
                }
                if !success {
                    break;
                }
            }
            if !success {
                return Err(format!(
                    "Upload file {} failed.",
                    file_info.depot_path.to_string()
                ));
            } else {
                // --- 文件切块完成，收集文件 FileChunks 信息 ---
                let submit_file = FileChunks {
                    path: file_info.depot_path.to_string(), // 使用服务器路径
                    binary_id: chunk_hashes,                // 块 Hash 列表
                };
                file_chunks.lock().await.push(submit_file);
            }
        } else {
            break;
        }
    }

    drop(marker);
    Ok(())
}
