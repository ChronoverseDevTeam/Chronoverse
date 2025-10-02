use tonic::{Request, Response, Status};
use crate::pb::{LoginReq, LoginRsp, RegisterReq, RegisterRsp};
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

pub async fn register(
    request: Request<RegisterReq>
) -> Result<Response<RegisterRsp>, Status> {
    let req = request.into_inner();
    if req.username.trim().is_empty() || req.password.is_empty() {
        return Err(Status::invalid_argument("username and password required"));
    }
    if crate::user::get_user_by_name(&req.username).await.map_err(|_| Status::internal("db error"))?.is_some() {
        return Err(Status::already_exists("username exists"));
    }

    let password = hash_password(&req.password).map_err(|_| Status::internal("hash password failed"))?;
    let now = chrono::Utc::now();
    let user = crate::user::UserEntity {
        name: req.username,
        email: req.email,
        password,
        created_at: now,
        updated_at: now,
    };
    crate::user::create_user(user).await.map_err(|_| Status::internal("db error"))?;
    Ok(Response::new(RegisterRsp {}))
}

fn hash_password(plain: &str) -> Result<String, argon2::password_hash::Error> {
    use argon2::{Argon2, PasswordHasher};
    use argon2::password_hash::{SaltString};
    let salt = SaltString::generate(&mut rand::thread_rng());
    let hashed = Argon2::default().hash_password(plain.as_bytes(), &salt)?.to_string();
    Ok(hashed)
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


