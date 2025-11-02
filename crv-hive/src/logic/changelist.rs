use tonic::{Request, Response, Status};
use crate::pb::{
    TrackFilesIntoChangelistReq, 
    UntrackFilesFromChangelistReq, 
    SubmitFilesInChangelistReq,
    SubmitFilesInChangelistRsp,
    ListFilesInChangelistReq,
    ListFilesInChangelistRsp,
    NilRsp,
    SubmitChangelistResult,
};
use crate::middleware::UserContext;
use chrono::Utc;

/// 辅助函数：验证工作区权限
async fn verify_workspace_permission(
    workspace_name: &str,
    username: &str,
) -> Result<(), Status> {
    let workspace = crate::database::workspace::get_workspace_by_name(workspace_name)
        .await
        .map_err(|e| Status::internal(format!("database error: {}", e)))?
        .ok_or_else(|| Status::not_found("workspace not found"))?;

    if workspace.owner != username {
        return Err(Status::permission_denied("you are not the owner of this workspace"));
    }

    Ok(())
}

/// 添加文件到变更列表（支持通用 changelist_id，0 代表默认）
pub async fn track_files_into_changelist(
    request: Request<TrackFilesIntoChangelistReq>
) -> Result<Response<NilRsp>, Status> {
    let user = request
        .extensions()
        .get::<UserContext>()
        .ok_or_else(|| Status::unauthenticated("missing user"))?
        .clone();
    let username = &user.username;
    let req = request.into_inner();

    // 验证请求参数
    if req.workspace_name.trim().is_empty() {
        return Err(Status::invalid_argument("workspace_name is required"));
    }
    if req.depot_paths.is_empty() {
        return Err(Status::invalid_argument("at least one file path is required"));
    }

    // 验证工作区权限
    verify_workspace_permission(&req.workspace_name, username).await?;

    // 为每个文件创建一个新的 revision
    // TODO: 实际场景中应该从请求中获取完整的 revision 信息
    let mut files_to_add = Vec::new();
    for depot_path in req.depot_paths {
        let revision = crv_core::metadata::file_revision::MetaFileRevision {
            revision: 1, // 临时版本号
            related_changelist_id: req.changelist_id as u64,
            block_hashes: vec![], // 应该从实际文件计算得出
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        files_to_add.push((depot_path, revision));
    }

    // 根据 changelist_id 选择操作方式
    if req.changelist_id == 0 {
        // 操作默认变更列表
        crate::database::changelist::add_files_to_default_changelist(
            &req.workspace_name,
            username,
            files_to_add
        )
        .await
        .map_err(|e| Status::internal(format!("failed to add files: {}", e)))?;
    } else {
        // 操作指定的变更列表
        return Err(Status::unimplemented("tracking files to non-default changelist is not yet supported"));
    }

    Ok(Response::new(NilRsp {}))
}

/// 从变更列表移除文件（支持通用 changelist_id，0 代表默认）
pub async fn untrack_files_from_changelist(
    request: Request<UntrackFilesFromChangelistReq>
) -> Result<Response<NilRsp>, Status> {
    let user = request
        .extensions()
        .get::<UserContext>()
        .ok_or_else(|| Status::unauthenticated("missing user"))?
        .clone();
    let username = &user.username;
    let req = request.into_inner();

    // 验证请求参数
    if req.workspace_name.trim().is_empty() {
        return Err(Status::invalid_argument("workspace_name is required"));
    }

    // 验证工作区权限
    verify_workspace_permission(&req.workspace_name, username).await?;

    // 根据 changelist_id 选择操作方式
    if req.changelist_id == 0 {
        // 操作默认变更列表
        if req.all.unwrap_or(false) {
            // 清空所有文件
            crate::database::changelist::clear_default_changelist(
                &req.workspace_name,
                username
            )
            .await
            .map_err(|e| Status::internal(format!("failed to clear files: {}", e)))?;
        } else {
            // 移除指定文件
            if req.depot_paths.is_empty() {
                return Err(Status::invalid_argument("depot_paths is required when all=false"));
            }

            crate::database::changelist::remove_files_from_default_changelist(
                &req.workspace_name,
                username,
                req.depot_paths
            )
            .await
            .map_err(|e| Status::internal(format!("failed to remove files: {}", e)))?;
        }
    } else {
        // 操作指定的变更列表
        return Err(Status::unimplemented("untracking files from non-default changelist is not yet supported"));
    }

    Ok(Response::new(NilRsp {}))
}

/// 列出变更列表中的文件（支持通用 changelist_id，0 代表默认）
pub async fn list_files_in_changelist(
    request: Request<ListFilesInChangelistReq>
) -> Result<Response<ListFilesInChangelistRsp>, Status> {
    let user = request
        .extensions()
        .get::<UserContext>()
        .ok_or_else(|| Status::unauthenticated("missing user"))?
        .clone();
    let username = &user.username;
    let req = request.into_inner();

    // 验证请求参数
    if req.workspace_name.trim().is_empty() {
        return Err(Status::invalid_argument("workspace_name is required"));
    }

    // 验证工作区权限
    verify_workspace_permission(&req.workspace_name, username).await?;

    // 根据 changelist_id 获取变更列表
    let changelist = if req.changelist_id == 0 {
        // 获取默认变更列表
        crate::database::changelist::get_default_changelist(
            &req.workspace_name,
            username
        )
        .await
        .map_err(|e| Status::internal(format!("database error: {}", e)))?
    } else {
        // 获取指定 ID 的变更列表
        crate::database::changelist::get_changelist_by_id(req.changelist_id as u64)
        .await
        .map_err(|e| Status::internal(format!("database error: {}", e)))?
    };

    // 如果变更列表不存在，返回空列表
    let depot_paths = if let Some(cl) = changelist {
        // 验证变更列表属于当前用户和工作区
        if cl.owner != *username || cl.workspace_name != req.workspace_name {
            return Err(Status::permission_denied("changelist does not belong to you or this workspace"));
        }
        cl.file_paths().into_iter().cloned().collect()
    } else {
        vec![]
    };

    Ok(Response::new(ListFilesInChangelistRsp {
        workspace_name: req.workspace_name,
        changelist_id: req.changelist_id,
        depot_paths,
    }))
}

/// 提交变更列表中的文件
/// 关键逻辑：提交时会生成新的 changelist id（> 0），原 changelist 会被清空或删除
pub async fn submit_files_in_changelist(
    request: Request<SubmitFilesInChangelistReq>
) -> Result<Response<SubmitFilesInChangelistRsp>, Status> {
    let user = request
        .extensions()
        .get::<UserContext>()
        .ok_or_else(|| Status::unauthenticated("missing user"))?
        .clone();
    let username = &user.username;
    let req = request.into_inner();

    // 验证请求参数
    if req.workspace_name.trim().is_empty() {
        return Err(Status::invalid_argument("workspace_name is required"));
    }
    if req.description.trim().is_empty() {
        return Err(Status::invalid_argument("description is required"));
    }

    // 验证工作区权限
    verify_workspace_permission(&req.workspace_name, username).await?;

    // 根据 changelist_id 获取要提交的变更列表
    let source_changelist = if req.changelist_id == 0 {
        // 获取默认变更列表
        crate::database::changelist::get_default_changelist(
            &req.workspace_name,
            username
        )
        .await
        .map_err(|e| Status::internal(format!("database error: {}", e)))?
        .ok_or_else(|| Status::not_found("default changelist not found or empty"))?
    } else {
        // 获取指定 ID 的变更列表
        let cl = crate::database::changelist::get_changelist_by_id(req.changelist_id as u64)
            .await
            .map_err(|e| Status::internal(format!("database error: {}", e)))?
            .ok_or_else(|| Status::not_found("changelist not found"))?;
        
        // 验证所有权
        if cl.owner != *username || cl.workspace_name != req.workspace_name {
            return Err(Status::permission_denied("changelist does not belong to you or this workspace"));
        }
        
        // 检查是否已经提交
        if cl.is_submitted() {
            return Err(Status::failed_precondition("changelist has already been submitted"));
        }
        
        cl
    };

    // 检查是否有文件需要提交
    if source_changelist.is_empty() {
        return Err(Status::failed_precondition("no files to submit"));
    }

    // 获取下一个可用的变更列表 ID（必须 > 0）
    let new_id = crate::database::changelist::get_next_changelist_id()
        .await
        .map_err(|e| Status::internal(format!("failed to get next changelist id: {}", e)))?;

    if new_id == 0 {
        return Err(Status::internal("invalid changelist id generated"));
    }

    // 创建已提交的新变更列表
    let submitted_cl = crv_core::metadata::changelist::Changelist {
        id: new_id,
        description: req.description,
        created_at: source_changelist.created_at,
        submitted_at: Some(Utc::now()),
        owner: username.clone(),
        workspace_name: req.workspace_name.clone(),
        files: source_changelist.files.clone(),
    };

    // 保存已提交的变更列表
    crate::database::changelist::create_changelist(submitted_cl)
        .await
        .map_err(|e| Status::internal(format!("failed to create submitted changelist: {}", e)))?;

    // 清理原变更列表
    if req.changelist_id == 0 {
        // 清空默认变更列表（不删除）
        crate::database::changelist::clear_default_changelist(
            &req.workspace_name,
            username
        )
        .await
        .map_err(|e| Status::internal(format!("failed to clear default changelist: {}", e)))?;
    } else {
        // 删除原变更列表
        crate::database::changelist::delete_changelist(req.changelist_id as u64)
            .await
            .map_err(|e| Status::internal(format!("failed to delete original changelist: {}", e)))?;
    }

    Ok(Response::new(SubmitFilesInChangelistRsp {
        changelist_id: new_id as i64,
        result: SubmitChangelistResult::Success as i32,
    }))
}
