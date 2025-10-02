use tonic::{Request, Status};

#[derive(Clone, Debug)]
pub struct UserContext {
    pub username: String,
    pub scopes: Vec<String>,
    pub source: &'static str,
}

// 服务器拦截器：仅校验 JWT；PAT 在各 RPC 内部异步校验
pub fn check_auth(mut req: Request<()>) -> Result<Request<()>, Status> {
    let md = req.metadata();

    if let Some(hv) = md.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = hv.strip_prefix("Bearer ") {
            if let Ok((username, scopes)) = verify_jwt_and_extract(token) {
                req.extensions_mut().insert(UserContext { username, scopes, source: "jwt" });
            } else {
                return Err(Status::unauthenticated("invalid bearer token"));
            }
        }
    }

    Ok(req)
}

fn verify_jwt_and_extract(token: &str) -> Result<(String, Vec<String>), ()> {
    #[derive(serde::Deserialize)]
    struct Claims { sub: String, exp: i64, scopes: Option<Vec<String>> }
    use jsonwebtoken::{DecodingKey, Validation, decode, Algorithm};
    let key = std::env::var("JWT_SECRET").map_err(|_| ())?;
    let data = decode::<Claims>(token, &DecodingKey::from_secret(key.as_bytes()), &Validation::new(Algorithm::HS256)).map_err(|_| ())?;
    Ok((data.claims.sub, data.claims.scopes.unwrap_or_default()))
}

fn find_pat_owner_and_scopes(_token_plain: &str) -> Option<(String, Vec<String>)> { None }


