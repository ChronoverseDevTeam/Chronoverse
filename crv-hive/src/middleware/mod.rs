use tonic::{Request, Status};

#[derive(Clone, Debug)]
pub struct UserContext {
    pub username: String,
    pub scopes: Vec<String>,
    pub source: &'static str,
}

#[derive(Clone, Debug)]
pub struct RenewToken {
    pub token: String,
    pub expires_at: i64,
}

pub fn apply_renew_metadata<T>(renew: Option<RenewToken>, resp: &mut tonic::Response<T>) {
    if let Some(newt) = renew.as_ref() {
        if let Ok(v) = tonic::metadata::MetadataValue::try_from(newt.token.as_str()) {
            let _ = resp.metadata_mut().insert("x-renew-token", v);
        }
        if let Ok(v) =
            tonic::metadata::MetadataValue::try_from(newt.expires_at.to_string().as_str())
        {
            let _ = resp.metadata_mut().insert("x-renew-expires-at", v);
        }
    }
}

pub fn enforce_jwt<T>(mut req: Request<T>) -> Result<Request<T>, Status> {
    // 其余接口要求有效 JWT
    let md = req.metadata().clone();
    let hv = md
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Status::unauthenticated("missing authorization header"))?;

    let token = hv
        .strip_prefix("Bearer ")
        .ok_or_else(|| Status::unauthenticated("invalid authorization scheme"))?;

    let (username, scopes, exp) = verify_jwt_and_extract(token)
        .map_err(|_| Status::unauthenticated("invalid bearer token"))?;

    req.extensions_mut().insert(UserContext {
        username,
        scopes,
        source: "jwt",
    });

    // 剩余时间 ≤ 45 分钟则续签（新的有效期 2 小时）
    let now = chrono::Utc::now().timestamp();
    if exp - now <= 45 * 60 {
        if let Some((tk, new_exp)) = issue_jwt_from_req_meta(&req.extensions()) {
            req.extensions_mut().insert(RenewToken {
                token: tk,
                expires_at: new_exp,
            });
        }
    }

    Ok(req)
}

fn verify_jwt_and_extract(token: &str) -> Result<(String, Vec<String>, i64), ()> {
    #[derive(serde::Deserialize)]
    struct Claims {
        sub: String,
        exp: i64,
        scopes: Option<Vec<String>>,
    }
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
    let key = crate::config::holder::get_or_init_config()
        .jwt_secret
        .clone();
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(key.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|_| ())?;
    let now = chrono::Utc::now().timestamp();
    if data.claims.exp <= now {
        return Err(());
    }
    Ok((
        data.claims.sub,
        data.claims.scopes.unwrap_or_default(),
        data.claims.exp,
    ))
}

fn issue_jwt_from_req_meta(ext: &tonic::Extensions) -> Option<(String, i64)> {
    let ctx = ext.get::<UserContext>()?;
    issue_jwt(&ctx.username, &ctx.scopes, 2 * 60 * 60).ok()
}

fn issue_jwt(username: &str, scopes: &[String], ttl_secs: i64) -> Result<(String, i64), ()> {
    #[derive(serde::Serialize)]
    struct Claims {
        sub: String,
        exp: i64,
        scopes: Vec<String>,
    }
    let exp = chrono::Utc::now().timestamp() + ttl_secs;
    let claims = Claims {
        sub: username.to_string(),
        exp,
        scopes: scopes.to_vec(),
    };
    let key = crate::config::holder::get_or_init_config()
        .jwt_secret
        .clone();
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(key.as_bytes()),
    )
    .map_err(|_| ())?;
    Ok((token, exp))
}
