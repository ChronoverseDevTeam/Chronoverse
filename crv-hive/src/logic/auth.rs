use tonic::{Request, Response, Status};
use crate::pb::{LoginReq, LoginRsp};
use argon2::PasswordVerifier;

pub async fn login(
    request: Request<LoginReq>
) -> Result<Response<LoginRsp>, Status> {
    let req = request.into_inner();

    let user = crate::user::get_user_by_name(&req.username)
        .await
        .map_err(|_| Status::internal("db error"))?
        .ok_or_else(|| Status::unauthenticated("invalid username or password"))?;

    if !verify_password(&req.password, &user.password) {
        return Err(Status::unauthenticated("invalid username or password"));
    }

    let (token, exp) = issue_jwt(&user.name, &[], 2 * 60 * 60)
        .map_err(|_| Status::internal("issue token failed"))?;

    Ok(Response::new(LoginRsp { access_token: token, expires_at: exp }))
}

fn verify_password(plain: &str, stored: &str) -> bool {
    // 支持存储为 Argon2 哈希；若不是合法 PHC 字符串，则回退为明文比较（便于迁移）
    if let Ok(parsed) = argon2::password_hash::PasswordHash::new(stored) {
        argon2::Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok()
    } else {
        plain == stored
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
    scopes: Vec<String>,
}

fn issue_jwt(username: &str, scopes: &[String], ttl_secs: i64) -> Result<(String, i64), jsonwebtoken::errors::Error> {
    let exp = chrono::Utc::now().timestamp() + ttl_secs;
    let claims = Claims { sub: username.to_string(), exp, scopes: scopes.to_vec() };
    let key = std::env::var("JWT_SECRET").map_err(|_| jsonwebtoken::errors::ErrorKind::InvalidKeyFormat)?;
    let token = jsonwebtoken::encode(&jsonwebtoken::Header::default(), &claims, &jsonwebtoken::EncodingKey::from_secret(key.as_bytes()))?;
    Ok((token, exp))
}


