use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::db::file::FileMeta;
use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::handlers::utils::{expand_paths_to_files, normalize_paths};
use crate::daemon_server::state::AppState;
use crate::hive_pb::{
    GetFileTreeReq, file_tree_node::Node, hive_service_client::HiveServiceClient,
};
use crate::pb::{SyncFileUpdate, SyncMetadata, SyncProgress, SyncReq};
use crv_core::path::basic::{LocalPath, WorkspacePath};
use crv_core::path::engine::PathEngine;
use std::collections::HashMap;
use std::pin::Pin;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

pub type SyncProgressStream =
    Pin<Box<dyn Stream<Item = Result<SyncProgress, Status>> + Send + 'static>>;

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

    // 2. 规范化路径
    let local_paths = normalize_paths(
        &request_body.paths,
        &request_body.workspace_name,
        &workspace_meta.config,
    )?;

    // 3. 展开为文件列表
    let local_files = expand_paths_to_files(&local_paths);

    // 4. 转换为 depot paths
    let path_engine = PathEngine::new(workspace_meta.config.clone(), &request_body.workspace_name);
    let mut depot_paths = Vec::new();
    let mut local_to_workspace: HashMap<String, WorkspacePath> = HashMap::new();

    for file in &local_files {
        let local_path = LocalPath::parse(file).unwrap();
        let workspace_path = path_engine.local_path_to_workspace_path(&local_path);
        let depot_path = path_engine.mapping_local_path(&local_path);

        if let (Some(ws_path), Some(dp_path)) = (workspace_path, depot_path) {
            depot_paths.push(dp_path);
            local_to_workspace.insert(file.clone(), ws_path);
        }
    }

    // 5. 获取 HiveClient 并调用 get_file_tree
    // 构建 depot wildcard (暂时获取所有文件，后续可以优化为只获取需要的)
    let depot_wildcard = "//...".to_string();

    let file_tree_rsp = hive_client
        .get_file_tree(GetFileTreeReq {
            depot_wildcard,
            changelist_id: 0,
        })
        .await?
        .into_inner();

    // 6. 构建 depot_path -> file info(revision id & file size) 的映射
    let mut depot_file_map: HashMap<String, (String, i64)> = HashMap::new();

    fn traverse_tree(
        nodes: &[crate::hive_pb::FileTreeNode],
        current_path: &str,
        map: &mut HashMap<String, (String, i64)>,
    ) {
        for node in nodes {
            if let Some(ref n) = node.node {
                match n {
                    Node::File(file) => {
                        let full_path = if current_path.is_empty() {
                            format!("//{}", file.name)
                        } else {
                            format!("{}/{}", current_path, file.name)
                        };
                        // todo 从 File revisions 中得到 latest revision 的 revision id 和 file size
                        let latest_revision = file
                            .revisions
                            .iter()
                            .max_by_key(|x| x.1.changelist_id)
                            .expect(&format!(
                                "Cannot get revision history of file {}",
                                full_path
                            ));

                        map.insert(
                            full_path,
                            (
                                latest_revision.1.revision_id.clone(),
                                latest_revision.1.size,
                            ),
                        );
                    }
                    Node::Directory(dir) => {
                        let full_path = if current_path.is_empty() {
                            format!("//{}", dir.name)
                        } else {
                            format!("{}/{}", current_path, dir.name)
                        };
                        traverse_tree(&dir.children, &full_path, map);
                    }
                }
            }
        }
    }

    traverse_tree(&file_tree_rsp.file_tree_root, "", &mut depot_file_map);

    // 7. 创建进度流
    let (tx, rx) = tokio::sync::mpsc::channel(128);
    let state_clone = state.clone();
    let local_to_workspace_clone = local_to_workspace.clone();

    tokio::spawn(async move {
        // 发送元数据
        let total_files = depot_paths.len() as i32;
        let total_bytes: i64 = depot_paths
            .iter()
            .filter_map(|dp| depot_file_map.get(&dp.to_string()))
            .map(|(_, size)| *size)
            .sum();

        let _ = tx
            .send(Ok(SyncProgress {
                payload: Some(crate::pb::sync_progress::Payload::Metadata(SyncMetadata {
                    total_bytes_to_sync: total_bytes,
                    total_files_to_sync: total_files,
                })),
            }))
            .await;

        let mut bytes_completed = 0i64;

        // 8. 处理每个文件
        for depot_path in depot_paths {
            let depot_path_str = depot_path.to_string();

            if let Some((revision_id, size)) = depot_file_map.get(&depot_path_str) {
                // 找到对应的 workspace_path
                if let Some(workspace_path) = local_to_workspace_clone
                    .iter()
                    .find(|(_, ws_path)| {
                        // 通过比较 depot_path 来匹配
                        let engine = PathEngine::new(workspace_meta.config.clone(), &request_body.workspace_name);
                        if let Some(ws_local) = engine.workspace_path_to_local_path(ws_path) {
                            if let Some(ws_depot) = engine.mapping_local_path(&ws_local) {
                                return ws_depot.to_string() == depot_path_str;
                            }
                        }
                        false
                    })
                    .map(|(_, ws_path)| ws_path)
                {
                    // 保存到数据库
                    let file_meta = FileMeta {
                        latest_revision: revision_id.clone(),
                    };

                    if let Err(e) = state_clone
                        .db
                        .set_file_meta(workspace_path.clone(), file_meta)
                    {
                        let _ = tx
                            .send(Err(Status::internal(format!(
                                "Failed to save file meta: {}",
                                e
                            ))))
                            .await;
                        return;
                    }

                    bytes_completed += size;

                    // 发送进度更新
                    let _ = tx
                        .send(Ok(SyncProgress {
                            payload: Some(crate::pb::sync_progress::Payload::FileUpdate(
                                SyncFileUpdate {
                                    path: depot_path_str.clone(),
                                    action: "synced".to_string(),
                                    bytes_completed_so_far: bytes_completed,
                                },
                            )),
                        }))
                        .await;
                }
            }
        }
    });

    // 9. 返回流
    let output_stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    Ok(Response::new(Box::pin(output_stream) as SyncProgressStream))
}
