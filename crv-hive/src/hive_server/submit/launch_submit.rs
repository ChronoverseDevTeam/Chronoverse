use crate::auth::require_user;
use crate::hive_server::submit::{SubmitManager, submit_manager};
use crate::hive_server::{derive_file_id_from_path, hive_dao};
use crate::pb::{LaunchSubmitReq, LaunchSubmitRsp};
use chrono::Duration;
use tonic::{Request, Response, Status};

async fn get_current_revision_id_at_head(
    branch_id: &str,
    file_id: &str,
) -> Result<Option<String>, Status> {
    let branch = hive_dao::find_branch_by_id(branch_id)
        .await
        .map_err(|e| Status::internal(format!("find_branch_by_id failed: {e}")))?
        .ok_or_else(|| Status::not_found(format!("branch not found: {branch_id}")))?;

    let mut current_id = branch.head_changelist_id;
    let mut steps: u32 = 0;

    while current_id > 0 {
        let cl_opt = hive_dao::find_changelist_by_id(current_id)
            .await
            .map_err(|e| Status::internal(format!("find_changelist_by_id failed: {e}")))?;

        let Some(cl) = cl_opt else { break };
        if cl.branch_id != branch_id {
            break;
        }

        for change in &cl.changes {
            if change.file != file_id {
                continue;
            }

            return Ok(match change.action {
                crv_core::metadata::ChangelistAction::Delete => None,
                crv_core::metadata::ChangelistAction::Create
                | crv_core::metadata::ChangelistAction::Modify => Some(change.revision.clone()),
            });
        }

        if cl.parent_changelist_id <= 0 {
            break;
        }
        current_id = cl.parent_changelist_id;

        steps += 1;
        if steps > 1_000_000 {
            break;
        }
    }

    Ok(None)
}

pub async fn handle_launch_submit(
    request: Request<LaunchSubmitReq>,
) -> Result<Response<LaunchSubmitRsp>, Status> {
    let _ = require_user(&request)?;

    let req = request.into_inner();

    // 默认 ticket 超时：客户端未提供字段，因此由服务器控制。
    // 如果你有全局配置项，我们也可以把这里改成从配置读取。
    const DEFAULT_TICKET_TIMEOUT_SECONDS: i64 = 30;
    let ticket = submit_manager().create_ticket(Duration::seconds(DEFAULT_TICKET_TIMEOUT_SECONDS));

    let files_to_lock = req.files.iter().map(|file| file.path.clone()).collect();
    if let Err(e) = submit_manager().batch_lock_files(&req.branch_id, &files_to_lock, ticket.clone()) {
        // 锁定失败不应留下无效 ticket
        let _ = submit_manager().cancel_ticket(&ticket);
        return Err(Status::internal(e.to_string()));
    }

    // todo: REVIEW starts

    // 第二步：对比当前文件 revision 与请求期望 revision 的差距，不一致则直接报错
    for file in &req.files {
        let file_id = derive_file_id_from_path(&file.path);
        let current_rev = get_current_revision_id_at_head(&req.branch_id, &file_id).await?;

        // 删除请求：expected_file_not_exist 无效，必须校验 expected_file_revision
        if file.is_delete {
            let expected = file.expected_file_revision.trim();
            if expected.is_empty() {
                let _ = submit_manager().cancel_ticket(&ticket);
                return Err(Status::failed_precondition(format!(
                    "expected_file_revision is required for delete: path={} branch_id={}",
                    file.path, req.branch_id
                )));
            }

            let Some(cur) = current_rev else {
                let _ = submit_manager().cancel_ticket(&ticket);
                return Err(Status::failed_precondition(format!(
                    "cannot delete non-existing file: path={} branch_id={}",
                    file.path, req.branch_id
                )));
            };

            if cur != expected {
                let _ = submit_manager().cancel_ticket(&ticket);
                return Err(Status::failed_precondition(format!(
                    "file revision mismatch (delete): path={} branch_id={} current={} expected={}",
                    file.path, req.branch_id, cur, expected
                )));
            }

            continue;
        }

        // 期望文件不存在
        if file.expected_file_not_exist {
            if let Some(cur) = current_rev {
                let _ = submit_manager().cancel_ticket(&ticket);
                return Err(Status::failed_precondition(format!(
                    "file should not exist: path={} branch_id={} current={}",
                    file.path, req.branch_id, cur
                )));
            }
            continue;
        }

        // 普通创建/修改：
        // - 若文件当前不存在：只有在“第一次创建”时 expected_file_revision 才允许为空
        // - 若文件当前存在：expected_file_revision 必须非空且一致
        match current_rev {
            None => {
                let expected = file.expected_file_revision.trim();
                if !expected.is_empty() {
                    let _ = submit_manager().cancel_ticket(&ticket);
                    return Err(Status::failed_precondition(format!(
                        "file revision mismatch (create): path={} branch_id={} current=<not-exist> expected={}",
                        file.path, req.branch_id, expected
                    )));
                }
            }
            Some(cur) => {
                let expected = file.expected_file_revision.trim();
                if expected.is_empty() {
                    let _ = submit_manager().cancel_ticket(&ticket);
                    return Err(Status::failed_precondition(format!(
                        "expected_file_revision is required: path={} branch_id={} current={}",
                        file.path, req.branch_id, cur
                    )));
                }
                if cur != expected {
                    let _ = submit_manager().cancel_ticket(&ticket);
                    return Err(Status::failed_precondition(format!(
                        "file revision mismatch: path={} branch_id={} current={} expected={}",
                        file.path, req.branch_id, cur, expected
                    )));
                }
            }
        }
    }

    // todo: REWVIEW ends

    Ok(Response::new(LaunchSubmitRsp {
        ticket,
        success: true,
        file_unable_to_lock: Vec::new(),
    }))
}