use crate::auth::require_user;
use crate::common::depot_path::DepotPath;
use crate::hive_server::submit::service::LockedFile;
use crate::hive_server::submit::submit_service;
use crate::logging::HiveLog;
use crate::pb::{FileUnableToLock, LaunchSubmitReq, LaunchSubmitRsp};
use tonic::{Request, Response, Status};

pub async fn handle_launch_submit(
    log: HiveLog,
    r: Request<LaunchSubmitReq>,
) -> Result<Response<LaunchSubmitRsp>, Status> {
    let user = require_user(&r)?;
    let submitting_by = user.username.clone();
    let log = log.with_user(&submitting_by);
    let _g = log.enter();

    let request = r.into_inner();
    log.info(&format!(
        "launch_submit received: files={}",
        request.files.len()
    ));

    let locked_files: Vec<LockedFile> = request
        .files
        .iter()
        .map(|file| {
            let path = DepotPath::new(&file.path).map_err(|e| {
                Status::invalid_argument(format!("invalid depot path '{}': {e}", file.path))
            })?;

            Ok(LockedFile {
                path,
                locked_generation: file.expected_file_generation,
                locked_revision: file.expected_file_revision,
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;

    let result = submit_service()
        .launch_submit(
            &locked_files,
            submitting_by,
            // 目前默认允许提交 2 小时
            chrono::Duration::hours(2),
        )
        .await;

    let rsp = match result {
        Ok(success) => LaunchSubmitRsp {
            ticket: success.ticket.to_string(),
            success: true,
            file_unable_to_lock: Vec::new(),
        },
        Err(failure) => {
            log.warn(&format!(
                "launch_submit failed: unable_to_lock={}",
                failure.file_unable_to_lock.len()
            ));
            let file_unable_to_lock = failure
                .file_unable_to_lock
                .iter()
                .map(|f| {
                    let expected_file_not_exist =
                        f.locked_generation.is_none() && f.locked_revision.is_none();
                    let expected_file_revision = match (f.locked_generation, f.locked_revision) {
                        (Some(g), Some(r)) => format!("{g}:{r}"),
                        (None, None) => String::new(),
                        _ => "invalid".to_string(),
                    };

                    FileUnableToLock {
                        path: f.path.to_string(),
                        current_file_revision: String::new(),
                        expected_file_revision,
                        expected_file_not_exist,
                    }
                })
                .collect();

            LaunchSubmitRsp {
                ticket: String::new(),
                success: false,
                file_unable_to_lock,
            }
        }
    };

    Ok(Response::new(rsp))
}