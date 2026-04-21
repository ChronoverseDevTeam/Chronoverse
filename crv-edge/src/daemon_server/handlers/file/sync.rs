use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::db::active_file::Action;
use crate::daemon_server::db::file::{FileGuard, FileLocation, FileMeta, FileRevision};
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{
    expand_to_mapped_files_in_edge_meta, filter_depot_paths, normalize_path,
};
use crate::daemon_server::job::{
    Job, JobEvent, JobRetentionPolicy, JobStatus, MessageStoragePolicy, WorkerProtocol,
};
use crate::daemon_server::state::AppState;
use crate::hive_pb::{
    DownloadFileChunkReq, GetFileTreeReq, hive_service_client::HiveServiceClient,
};
use crate::pb::sync_progress::Payload::FileUpdate;
use crate::pb::{SyncFileUpdate, SyncProgress, SyncReq};
use crv_core::path::basic::DepotPath;
use crv_core::path::engine::PathEngine;
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::ops::Sub;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

pub type SyncProgressStream =
    Pin<Box<dyn Stream<Item = Result<SyncProgress, Status>> + Send + 'static>>;

struct JobCancelOnDropStream {
    stream: SyncProgressStream,
    job: Weak<Job>,
}

impl Stream for JobCancelOnDropStream {
    type Item = Result<SyncProgress, Status>;

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

struct FileToSync {
    location: FileLocation,
    action: Action,
    // None only when action is Delete
    latest_revision: Option<FileRevision>,
    chunk_hashes: Vec<String>,
}

const FRAME_SIZE: usize = 64 * 1024; // 64KB，单个报文中的数据大小

pub async fn handle(
    state: AppState,
    req: Request<SyncReq>,
) -> AppResult<Response<SyncProgressStream>> {
    let runtime_config = RuntimeConfig::from_req(&req)?;
    let request_body = req.into_inner();

    let channel = state
        .hive_channel
        .get_channel(&runtime_config.remote_addr.value)?;

    let mut hive_client = HiveServiceClient::new(channel.clone());

    // 1. 获取 workspace 信息
    let workspace_meta = state
        .db
        .get_confirmed_workspace_meta(&request_body.workspace_name)?
        .ok_or(AppError::Raw(Status::not_found(format!(
            "Workspace {} not found.",
            request_body.workspace_name
        ))))?;

    let path_engine = PathEngine::new(workspace_meta.config.clone(), &request_body.workspace_name);

    // 规范化路径
    let mut edge_files = vec![];
    let mut location_unions = vec![];
    for path in &request_body.paths {
        let location_union = normalize_path(path, &path_engine)?;
        edge_files.extend(expand_to_mapped_files_in_edge_meta(
            &location_union,
            &path_engine,
            state.clone(),
        )?);
        location_unions.push(location_union);
    }

    // 4. 获取 hive files
    // 构建 depot wildcard (暂时获取所有文件，后续可以优化为只获取需要的)
    let depot_wildcard = "//...".to_string();

    let file_tree_rsp = hive_client
        .get_file_tree(GetFileTreeReq {
            depot_wildcard,
            changelist_id: 0,
        })
        .await?
        .into_inner();

    // 5. 构建 FileToSync 列表，此时无法保证文件是未 checkout 的状态
    let mut file_to_sync = vec![];
    let edge_files_map = edge_files
        .iter()
        .map(|x| (x.depot_path.to_custom_string(), x))
        .collect::<HashMap<_, _>>();

    // 这个过程获取到的文件不一定都在参数指定的文件范围内，比如排除文件没办法静态计算
    let hive_files_map = file_tree_rsp
        .file_revisions
        .iter()
        .filter_map(|x| {
            if x.generation == 0 && x.revision == 0 {
                None
            } else {
                Some((x.path.clone(), x))
            }
        })
        .collect::<HashMap<_, _>>();

    // 因此，这里需要过滤一下
    let hive_depot_paths_candidates = hive_files_map
        .keys()
        .map(|x| DepotPath::parse(x).unwrap())
        .collect::<Vec<_>>();
    let mut hive_depot_paths = vec![];

    for location_union in &location_unions {
        hive_depot_paths.append(&mut filter_depot_paths(
            location_union,
            &hive_depot_paths_candidates,
            &path_engine,
        ));
    }

    let hive_files_set = hive_depot_paths
        .iter()
        .map(|x| x.depot_path.to_custom_string())
        .collect::<HashSet<_>>();
    let edge_files_set = edge_files_map.keys().cloned().collect::<HashSet<_>>();

    let files_to_add = hive_files_set.sub(&edge_files_set);
    let files_to_delete = edge_files_set.sub(&hive_files_set);
    let mut files_to_edit = HashSet::new();
    for (k, v) in edge_files_map.iter() {
        if files_to_add.contains(k) || files_to_delete.contains(k) {
            continue;
        }
        let file_meta = state.db.get_file_meta(&v.workspace_path)?;
        if file_meta.is_none() {
            continue;
        }
        let edge_file_meta = file_meta.unwrap();
        let hive_file_meta = hive_files_map.get(k).unwrap();
        if edge_file_meta.current_revision.generation == hive_file_meta.generation
            && edge_file_meta.current_revision.revision == hive_file_meta.revision
        {
            continue;
        }
        files_to_edit.insert(k.clone());
    }

    // 至此，files_to_add、files_to_edit 和 files_to_delete 才确切可用
    let mut add_locations = vec![];
    for file in &files_to_add {
        let file_meta = hive_files_map.get(file).unwrap();
        let depot_path = DepotPath::parse(&file_meta.path).unwrap();
        let local_path = path_engine.mapping_depot_path(&depot_path);
        if local_path.is_none() {
            continue;
        }
        let local_path = local_path.unwrap();
        let workspace_path = path_engine
            .local_path_to_workspace_path(&local_path)
            .unwrap();

        add_locations.push(FileLocation {
            local_path,
            workspace_path,
            depot_path,
        });
    }

    let mut existing_locations = vec![];
    for file in files_to_edit.iter().chain(files_to_delete.iter()) {
        let location = edge_files_map.get(file).unwrap();
        existing_locations.push((*location).clone());
    }

    let file_guard = state.db.prepare_command(&add_locations, &existing_locations)?;

    let prepared_add_paths = file_guard
        .add_paths
        .iter()
        .map(|x| x.to_custom_string())
        .collect::<HashSet<_>>();
    let prepared_existing_paths = file_guard
        .existing_paths
        .iter()
        .map(|x| x.to_custom_string())
        .collect::<HashSet<_>>();

    // 从 hive file map 中获取元数据
    for location in add_locations {
        if !prepared_add_paths.contains(&location.workspace_path.to_custom_string()) {
            continue;
        }

        let file_meta = hive_files_map
            .get(&location.depot_path.to_custom_string())
            .unwrap();
        file_to_sync.push(FileToSync {
            location,
            action: Action::Add,
            latest_revision: Some(FileRevision {
                generation: file_meta.generation,
                revision: file_meta.revision,
            }),
            chunk_hashes: file_meta.binary_id.clone(),
        });
    }

    for file in &files_to_edit {
        let location = edge_files_map.get(file).unwrap();
        if !prepared_existing_paths.contains(&location.workspace_path.to_custom_string()) {
            continue;
        }

        let file_meta = hive_files_map.get(file).unwrap();
        file_to_sync.push(FileToSync {
            location: (*location).clone(),
            action: Action::Edit,
            latest_revision: Some(FileRevision {
                generation: file_meta.generation,
                revision: file_meta.revision,
            }),
            chunk_hashes: file_meta.binary_id.clone(),
        });
    }

    // 从 edge file map 中获取元数据
    for file in files_to_delete {
        let location = edge_files_map.get(&file).unwrap();
        if !prepared_existing_paths.contains(&location.workspace_path.to_custom_string()) {
            continue;
        }

        file_to_sync.push(FileToSync {
            location: (*location).clone(),
            action: Action::Delete,
            latest_revision: None,
            chunk_hashes: vec![],
        });
    }

    // 7. 创建 Job
    let job = state.job_manager.create_job(
        None,
        MessageStoragePolicy::None,
        WorkerProtocol::And,
        JobRetentionPolicy::Immediate,
    );
    let rx = job.tx.subscribe();

    let state_clone = state.clone();

    // 8. 添加 Worker
    let job_ref = job.clone();
    job.add_worker(async move {
        sync_file(state_clone, file_to_sync, channel, file_guard, job_ref).await
    });

    job.clone().start();

    // 9. 构建输出流
    let output_stream = BroadcastStream::new(rx).filter_map(move |res| match res {
        Ok(event) => match event {
            JobEvent::Payload(any) => SyncProgress::decode(&any.value[..]).ok().map(Ok),
            JobEvent::Error(e) => Some(Err(Status::internal(e))),
            JobEvent::StatusChange(JobStatus::Failed(e)) => Some(Err(Status::internal(e))),
            JobEvent::StatusChange(JobStatus::Cancelled) => {
                Some(Err(Status::cancelled("Sync cancelled")))
            }
            _ => None,
        },
        Err(_) => Some(Err(Status::internal("Stream lagged"))),
    });

    let wrapped_stream = JobCancelOnDropStream {
        stream: Box::pin(output_stream),
        job: Arc::downgrade(&job),
    };

    Ok(Response::new(Box::pin(wrapped_stream) as SyncProgressStream))
}

async fn sync_file(
    app_state: AppState,
    files_to_sync: Vec<FileToSync>,
    channel: Channel,
    _file_guard: FileGuard,
    job: Arc<Job>,
) -> Result<(), String> {
    let mut hive_client = HiveServiceClient::new(channel.clone());
    for file in files_to_sync {
        // 对于本地 checkout 的文件，跳过该文件的拉新。
        if app_state
            .db
            .get_active_file_action(&file.location.workspace_path)
            .map_err(|x| format!("{x}"))?
            .is_some()
        {
            println!(
                "Already checkout file {}, skip sync.",
                file.location.workspace_path.to_custom_string()
            );
            continue;
        }

        match file.action {
            Action::Add | Action::Edit => {
                let mut file_fs = fs::File::create(file.location.local_path.to_local_path_string())
                    .await
                    .map_err(|x| format!("{x}"))?;

                let mut bytes_completed_so_far = 0;

                for chunk_hash in file.chunk_hashes {
                    let download_file_chunk_req = DownloadFileChunkReq {
                        chunk_hashes: vec![chunk_hash],
                        packet_size: FRAME_SIZE as i64,
                    };
                    let mut download_file_chunk_rsp_stream = hive_client
                        .download_file_chunk(download_file_chunk_req)
                        .await
                        .map_err(|x| format!("{x}"))?
                        .into_inner();

                    while let Some(rsp) = download_file_chunk_rsp_stream
                        .message()
                        .await
                        .map_err(|x| format!("{x}"))?
                    {
                        assert_eq!(rsp.compression, "none");
                        file_fs.write_all(&rsp.content).await.unwrap();
                        bytes_completed_so_far += rsp.content.len();
                        job.report_payload(SyncProgress {
                            payload: Some(FileUpdate(SyncFileUpdate {
                                path: file.location.workspace_path.to_custom_string(),
                                bytes_completed_so_far: bytes_completed_so_far as i64,
                                info: "".to_string(),
                                warning: "".to_string(),
                            })),
                        })
                    }
                }
                let file_meta = FileMeta {
                    location: file.location,
                    current_revision: file.latest_revision.unwrap(),
                    busy: true, // 因为 sync 操作还没有完成，像 sync 这样的操作最好还是完全完成后再解锁
                };
                app_state
                    .db
                    .set_file_meta(file_meta.location.workspace_path.clone(), file_meta)
                    .map_err(|x| format!("{x}"))?;
            }
            Action::Delete => {
                app_state
                    .db
                    .delete_file_meta(&file.location.workspace_path)
                    .map_err(|x| format!("{x}"))?;
                fs::remove_file(file.location.local_path.to_local_path_string())
                    .await
                    .map_err(|x| format!("{x}"))?;
                job.report_payload(SyncProgress {
                    payload: Some(FileUpdate(SyncFileUpdate {
                        path: file.location.workspace_path.to_custom_string(),
                        bytes_completed_so_far: 0,
                        info: "Delete successfully.".to_string(),
                        warning: "".to_string(),
                    })),
                })
            }
        }
    }

    Ok(())
}
