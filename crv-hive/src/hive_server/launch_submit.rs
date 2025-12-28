use crate::hive_server::{depot_tree, hive_dao};
use crate::pb::{FileUnableToLock, LaunchSubmitReq, LaunchSubmitRsp};
use std::collections::HashSet;
use tonic::{Request, Response, Status};
use uuid::Uuid;

fn generate_ticket() -> String {
    // 生成 UUID 并去掉连字符
    Uuid::new_v4().to_string().replace('-', "")
}

pub async fn handle_launch_submit(
    request: Request<LaunchSubmitReq>,
) -> Result<Response<LaunchSubmitRsp>, Status> {
    let req = request.into_inner();
    let branch_id = req.branch_id;

    // 查询分支信息
    let branch = hive_dao::find_branch_by_id(&branch_id)
        .await
        .map_err(|e| Status::internal(format!("failed to find branch: {e}")))?
        .ok_or_else(|| Status::not_found(format!("branch not found: {branch_id}")))?;

    let head_cl_id = branch.head_changelist_id;

    // 获取 DepotTree 锁
    let depot = depot_tree();
    let mut depot_guard = depot.lock().await;

    // 收集所有文件 ID，用于批量锁定检查
    let file_ids: Vec<String> = req.files.iter().map(|f| f.file_id.clone()).collect();

    // 先尝试在 DepotTree 中锁定所有文件
    let (_locked_file_ids, conflicted_file_ids) = depot_guard.try_lock_files(&branch_id, &file_ids);

    // 如果有冲突，直接返回失败
    if !conflicted_file_ids.is_empty() {
        let conflicted_set: HashSet<String> = conflicted_file_ids.into_iter().collect();
        let mut unable_to_lock = Vec::new();

        for file in req.files {
            if conflicted_set.contains(&file.file_id) {
                unable_to_lock.push(FileUnableToLock {
                    file_id: file.file_id,
                    branch_id: branch_id.clone(),
                    path: file.path,
                    current_file_revision: String::new(),
                    expected_file_revision: file.expected_file_revision,
                    expected_file_not_exist: file.expected_file_not_exist,
                });
            }
        }

        return Ok(Response::new(LaunchSubmitRsp {
            ticket: String::new(),
            success: false,
            file_unable_to_lock: unable_to_lock,
        }));
    }

    // 验证每个文件的版本是否符合期望
    let mut unable_to_lock = Vec::new();

    for file in req.files {
        // 查询文件在当前分支 HEAD 的版本
        let current_revision = hive_dao::find_file_revision_by_branch_file_and_cl(
            &branch_id,
            &file.file_id,
            head_cl_id,
        )
        .await
        .map_err(|e| {
            Status::internal(format!(
                "failed to query file revision: {e}"
            ))
        })?;

        let file_exists = current_revision.is_some();
        let current_revision_id = current_revision.as_ref().map(|r| r.id.clone());

        // 验证文件状态是否符合期望
        // 注意：如果 is_delete 为 true，expected_file_not_exist 应该无效
        let is_valid = if file.is_delete {
            // 删除操作：文件必须存在
            file_exists
        } else if file.expected_file_not_exist {
            // 期望文件不存在（创建新文件）
            !file_exists
        } else if file.expected_file_revision.is_empty() {
            // 期望版本为空，表示第一次创建（文件应该不存在）
            !file_exists
        } else {
            // 期望版本不为空，需要匹配当前版本
            current_revision_id.as_ref() == Some(&file.expected_file_revision)
        };

        if !is_valid {
            // 文件无法锁定，需要解锁之前已锁定的文件
            unable_to_lock.push(FileUnableToLock {
                file_id: file.file_id.clone(),
                branch_id: branch_id.clone(),
                path: file.path.clone(),
                current_file_revision: current_revision_id.unwrap_or_default(),
                expected_file_revision: file.expected_file_revision,
                expected_file_not_exist: file.expected_file_not_exist,
            });
        }
    }

    // 如果有文件无法锁定，释放所有已锁定的文件并返回失败
    if !unable_to_lock.is_empty() {
        depot_guard.unlock_files(&branch_id, &file_ids);
        return Ok(Response::new(LaunchSubmitRsp {
            ticket: String::new(),
            success: false,
            file_unable_to_lock: unable_to_lock,
        }));
    }

    // 所有文件都成功锁定，生成 ticket
    let ticket = generate_ticket();

    Ok(Response::new(LaunchSubmitRsp {
        ticket,
        success: true,
        file_unable_to_lock: Vec::new(),
    }))
}
