use tonic::{Request, Response, Status};
use crate::pb::{CreateTokenReq, CreateTokenRsp, ListTokensReq, ListTokensRsp, RevokeTokenReq, RevokeTokenRsp, TokenInfo};
use crate::hive_server::auth::UserContext;

fn ensure_user<T>(req: &Request<T>) -> Result<&UserContext, Status> {
    req.extensions().get::<UserContext>().ok_or_else(|| Status::unauthenticated("missing user"))
}

pub async fn create_token(
    request: Request<CreateTokenReq>
) -> Result<Response<CreateTokenRsp>, Status> {
    let user = ensure_user(&request)?.clone();
    let req = request.into_inner();

    let (token_plain, token_sha256) = generate_pat();
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let expires_at = req.expires_at.map(|v| chrono::DateTime::from_timestamp(v, 0).unwrap());

    let entity = crate::tokens::PersonalToken {
        id: id.clone(),
        user: user.username,
        name: req.name,
        token_sha256,
        created_at: now,
        expires_at,
        scopes: req.scopes,
        last_used_at: None,
    };

    crate::tokens::insert(entity).await.map_err(|_| Status::internal("db error"))?;

    Ok(Response::new(CreateTokenRsp { token: token_plain, id }))
}

pub async fn list_tokens(
    request: Request<ListTokensReq>
) -> Result<Response<ListTokensRsp>, Status> {
    let user = ensure_user(&request)?;
    let items = crate::tokens::list_by_user(&user.username).await.map_err(|_| Status::internal("db error"))?;
    let tokens: Vec<TokenInfo> = items.into_iter().map(|t| TokenInfo {
        id: t.id,
        name: t.name,
        created_at: t.created_at.timestamp(),
        expires_at: t.expires_at.map(|d| d.timestamp()),
        scopes: t.scopes,
        last_used_at: t.last_used_at.map(|d| d.timestamp()),
    }).collect();
    Ok(Response::new(ListTokensRsp { tokens }))
}

pub async fn revoke_token(
    request: Request<RevokeTokenReq>
) -> Result<Response<RevokeTokenRsp>, Status> {
    let user = ensure_user(&request)?.clone();
    let req = request.into_inner();
    let ok = crate::tokens::delete_by_id(&user.username, &req.id).await.map_err(|_| Status::internal("db error"))?;
    if !ok { return Err(Status::not_found("token not found")); }
    Ok(Response::new(RevokeTokenRsp {}))
}

fn generate_pat() -> (String, String) {
    use rand::RngCore;
    use sha2::{Sha256, Digest};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let token_plain = URL_SAFE_NO_PAD.encode(&buf);
    let token_sha256 = URL_SAFE_NO_PAD.encode(Sha256::digest(token_plain.as_bytes()));
    (token_plain, token_sha256)
}


