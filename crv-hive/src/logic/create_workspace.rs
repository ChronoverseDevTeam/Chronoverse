use tonic::{Request, Response, Status};

use crate::{
    hive_server::auth::{apply_renew_metadata, RenewToken, UserContext},
    pb::{CreateWorkspaceReq, NilRsp},
};

pub async fn create_workspace(
    request: Request<CreateWorkspaceReq>,
) -> Result<Response<NilRsp>, Status> {
    let renew = request.extensions().get::<RenewToken>().cloned();

    let user = request
        .extensions()
        .get::<UserContext>()
        .ok_or_else(|| Status::unauthenticated("missing user"))?
        .clone();

    let req = request.into_inner();

    if req.name.trim().is_empty() {
        return Err(Status::invalid_argument("name required"));
    }
    if req.path.trim().is_empty() {
        return Err(Status::invalid_argument("path required"));
    }
    if req.device_finger_print.trim().is_empty() {
        return Err(Status::invalid_argument("device_finger_print required"));
    }

    let now = chrono::Utc::now();
    let entity = crate::workspace::WorkspaceEntity {
        name: req.name,
        created_at: now,
        updated_at: now,
        owner: user.username,
        path: req.path,
        device_finger_print: req.device_finger_print,
    };

    // 原子插入：依赖 MongoDB 唯一键（_id）冲突返回重复错误
    use mongodb::error::ErrorKind;
    if let Err(e) = crate::workspace::create_workspace(entity).await {
        // E11000 duplicate key error
        let is_dup = match e.kind.as_ref() {
            ErrorKind::Write(wf) => match wf {
                mongodb::error::WriteFailure::WriteError(we) => we.code == 11000,
                mongodb::error::WriteFailure::WriteConcernError(_) => false,
                _ => false,
            },
            ErrorKind::BulkWrite(bwe) => bwe
                .write_errors
                .iter()
                .any(|(_, we)| we.code == 11000),
            ErrorKind::Command(ce) => ce.code == 11000 || ce.code_name == "DuplicateKey",
            _ => false,
        };
        if is_dup {
            return Err(Status::already_exists("workspace exists"));
        }
        return Err(Status::internal("db error"));
    }

    let mut resp = Response::new(NilRsp {});
    apply_renew_metadata(renew, &mut resp);
    Ok(resp)
}
