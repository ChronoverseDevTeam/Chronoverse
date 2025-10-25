use tonic::{Request, Response, Status};

use crate::{
    middleware::UserContext,
    pb::{UpsertWorkspaceReq, NilRsp},
};

pub async fn upsert_workspace(
    request: Request<UpsertWorkspaceReq>,
) -> Result<Response<NilRsp>, Status> {
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
    let entity = crate::database::workspace::WorkspaceEntity {
        name: req.name,
        created_at: now,
        updated_at: now,
        owner: user.username,
        path: req.path,
        device_finger_print: req.device_finger_print,
    };

    // Upsert 逻辑：如果存在则更新，不存在则创建
    // 利用 MongoDB 的单文档原子性保证一致性
    crate::database::workspace::upsert_workspace(entity)
        .await
        .map_err(|_| Status::internal("db error"))?;

    Ok(Response::new(NilRsp {}))
}
