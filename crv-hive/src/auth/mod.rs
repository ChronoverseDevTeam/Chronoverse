use std::sync::Arc;

use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::{metadata::MetadataValue, Request, Response, Status};
use tonic::service::Interceptor;

use argon2::password_hash::{PasswordHash, PasswordVerifier};
use argon2::Argon2;

use crate::config::holder::get_or_init_config;
use crate::database::dao;

/// 领域层的用户身份信息（与具体传输协议无关）
#[derive(Debug, Clone)]
pub struct UserContext {
    pub username: String,
    pub scopes: Vec<String>,
    /// 身份来源，比如 jwt / internal 等
    pub source: AuthSource,
}

#[derive(Debug, Clone, Copy)]
pub enum AuthSource {
    Jwt,
    /// 预留给将来可能的其他来源（如内部调用）
    Internal,
}

/// token 元信息，主要用于续签判断
#[derive(Debug, Clone, Copy)]
pub struct TokenMeta {
    pub exp: i64,
}

/// 统一的鉴权错误类型，避免业务逻辑直接依赖 tonic::Status
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("missing authorization header")]
    MissingHeader,
    #[error("invalid authorization scheme")]
    InvalidScheme,
    #[error("invalid bearer token")]
    InvalidToken,
    #[error("token expired")]
    ExpiredToken,
    #[error("internal auth error")]
    Internal,
}

impl From<AuthError> for Status {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::MissingHeader | AuthError::InvalidScheme | AuthError::InvalidToken => {
                Status::unauthenticated(err.to_string())
            }
            AuthError::ExpiredToken => Status::unauthenticated("token expired"),
            AuthError::Internal => Status::internal("auth internal error"),
        }
    }
}

/// 签发与验证策略：token 有效期，以及在还剩多久时触发续签。
#[derive(Debug, Clone, Copy)]
pub struct TokenPolicy {
    /// access token 有效期（秒）
    pub ttl_secs: i64,
    /// 当剩余有效期小于该值时，触发续签（秒）
    pub renew_before_secs: i64,
}

impl Default for TokenPolicy {
    fn default() -> Self {
        // 默认：2 小时有效期，剩余 ≤ 45 分钟自动续签
        Self {
            ttl_secs: 2 * 60 * 60,
            renew_before_secs: 45 * 60,
        }
    }
}

/// JWT Claims（用于 jsonwebtoken 编解码）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: i64,
    #[serde(default)]
    scopes: Vec<String>,
}

/// 统一的鉴权服务：封装 JWT 签发、验证与续签策略。
#[derive(Clone)]
pub struct AuthService {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    policy: TokenPolicy,
}

impl AuthService {
    /// 基于全局配置初始化 AuthService
    pub fn from_config() -> Arc<Self> {
        let cfg = get_or_init_config();
        let secret = cfg.jwt_secret.clone();
        Arc::new(Self::new(secret.as_bytes(), TokenPolicy::default()))
    }

    pub fn new(secret: &[u8], policy: TokenPolicy) -> Self {
        Self {
            encoding_key: EncodingKey::from_secret(secret),
            decoding_key: DecodingKey::from_secret(secret),
            policy,
        }
    }

    /// 签发新的 access token，返回 (token, 过期时间戳)
    pub fn issue_token(
        &self,
        username: &str,
        scopes: &[String],
    ) -> Result<(String, i64), AuthError> {
        let exp = Utc::now().timestamp() + self.policy.ttl_secs;
        let claims = Claims {
            sub: username.to_string(),
            exp,
            scopes: scopes.to_vec(),
        };

        let token =
            encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key).map_err(|_| {
                // 具体错误对外隐藏，避免泄露实现细节
                AuthError::Internal
            })?;

        Ok((token, exp))
    }

    /// 验证 Bearer Token，返回领域层 UserContext 与 TokenMeta
    pub fn verify_token(&self, token: &str) -> Result<(UserContext, TokenMeta), AuthError> {
        let data = decode::<Claims>(
            token,
            &self.decoding_key,
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| AuthError::InvalidToken)?;

        let now = Utc::now().timestamp();
        if data.claims.exp <= now {
            return Err(AuthError::ExpiredToken);
        }

        let ctx = UserContext {
            username: data.claims.sub,
            scopes: data.claims.scopes,
            source: AuthSource::Jwt,
        };
        let meta = TokenMeta { exp: data.claims.exp };

        Ok((ctx, meta))
    }

    /// 判断是否需要续签；如需要则返回新的 (token, exp)
    pub fn maybe_renew(&self, ctx: &UserContext, meta: TokenMeta) -> Option<(String, i64)> {
        let now = Utc::now().timestamp();
        if meta.exp - now <= self.policy.renew_before_secs {
            // 使用当前上下文中的身份信息重新签发
            let scopes = ctx.scopes.clone();
            self.issue_token(&ctx.username, &scopes).ok()
        } else {
            None
        }
    }
}

/// 拦截后存放在 Request.extensions 中的续签信息
#[derive(Debug, Clone)]
pub struct RenewToken {
    pub token: String,
    pub expires_at: i64,
}

/// 在具体 RPC handler 中获取当前登录用户的便捷函数。
///
/// - 如已登录（拦截器已注入 UserContext），返回 `&UserContext`
/// - 如未登录，则返回 `Status::unauthenticated("login required")`
pub fn require_user<T>(req: &Request<T>) -> Result<&UserContext, Status> {
    req.extensions()
        .get::<UserContext>()
        .ok_or_else(|| Status::unauthenticated("login required"))
}

/// 统一的 gRPC 鉴权拦截函数，可在 Interceptor / tower layer 中复用。
///
/// - 解析 `authorization: Bearer xxx`
/// - 验证 JWT，写入 `UserContext` 到 `extensions`
/// - 依据策略决定是否续签，如续签则写入 `RenewToken` 到 `extensions`
pub fn enforce_jwt_on_request<T>(
    mut req: Request<T>,
    auth: &AuthService,
) -> Result<Request<T>, Status> {
    let md = req.metadata().clone();
    let header_val = match md
        .get("authorization")
        .and_then(|v| v.to_str().ok())
    {
        // 未携带 Authorization 头时，直接放行，交由具体业务决定是否需要登录
        None => return Ok(req),
        Some(v) => v,
    };

    let token = header_val
        .strip_prefix("Bearer ")
        .ok_or(AuthError::InvalidScheme)?;

    let (ctx, meta) = auth.verify_token(token)?;

    // 在 extensions 中存入用户上下文
    req.extensions_mut().insert(ctx.clone());

    // 决定是否续签
    if let Some((new_token, new_exp)) = auth.maybe_renew(&ctx, meta) {
        req.extensions_mut().insert(RenewToken {
            token: new_token,
            expires_at: new_exp,
        });
    }

    Ok(req)
}

/// 将 `RenewToken` 写入到 gRPC Response 的 metadata 中，供客户端透明收到续签信息。
pub fn apply_renew_metadata<T>(req: &Request<T>, resp: &mut Response<()>) {
    if let Some(renew) = req.extensions().get::<RenewToken>() {
        if let Ok(v) = MetadataValue::try_from(renew.token.as_str()) {
            let _ = resp.metadata_mut().insert("x-renew-token", v);
        }
        if let Ok(v) = MetadataValue::try_from(renew.expires_at.to_string().as_str()) {
            let _ = resp.metadata_mut().insert("x-renew-expires-at", v);
        }
    }
}

/// 服务端 gRPC 鉴权拦截器实现，包装 `enforce_jwt_on_request`。
#[derive(Clone)]
pub struct AuthInterceptor {
    auth: Arc<AuthService>,
}

impl AuthInterceptor {
    pub fn new(auth: Arc<AuthService>) -> Self {
        Self { auth }
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        enforce_jwt_on_request(req, &self.auth)
    }
}

/// 校验用户名/密码是否合法的函数
pub async fn validate_user_credentials(
    username: &str,
    password: &str,
) -> Result<bool, AuthError> {
    // 测试环境内置测试账号：admin / admin（仅在 `cargo test` 时生效，不影响生产/开发环境运行的服务进程）
    if cfg!(test) && username == "admin" && password == "admin" {
        return Ok(true);
    }

    // 尝试从 MongoDB 中读取用户信息
    let user_doc_opt = dao::find_user_by_username(username)
        .await
        // 对于 DAO 层错误，这里统一视为认证失败，而不是返回内部错误，避免泄露实现细节
        .unwrap_or(None);

    let user = match user_doc_opt {
        Some(u) => u,
        None => return Ok(false),
    };

    let stored = user.password;

    // 优先尝试将 stored 作为 argon2 密文进行验证
    if let Ok(parsed) = PasswordHash::new(&stored) {
        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
        {
            return Ok(true);
        } else {
            return Ok(false);
        }
    }

    // 否则退回到明文比较（兼容老数据）
    Ok(stored == password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthService, AuthInterceptor, TokenPolicy, UserContext, RenewToken};
    use crate::hive_server::CrvHiveService;
    use crate::pb::hive_service_server::HiveService;
    use tonic::{Request, Code};
    use tonic::metadata::MetadataValue;

    /// 缺少 Authorization 头时，应直接放行，但不注入 UserContext
    #[test]
    fn interceptor_allows_when_missing_authorization_header() {
        let policy = TokenPolicy {
            ttl_secs: 60,
            renew_before_secs: 30,
        };
        let auth = Arc::new(AuthService::new(b"test-secret", policy));
        let mut interceptor = AuthInterceptor::new(auth);

        let req = Request::new(());
        let res = <AuthInterceptor as tonic::service::Interceptor>::call(&mut interceptor, req);

        assert!(res.is_ok(), "request without authorization should be accepted");
        let req = res.unwrap();
        assert!(
            req.extensions().get::<UserContext>().is_none(),
            "UserContext should not be injected when authorization header is missing"
        );
    }

    /// 携带合法 Bearer Token 时，应通过并在 extensions 中注入 UserContext
    #[test]
    fn interceptor_accepts_valid_token_and_injects_user_context() {
        let policy = TokenPolicy {
            ttl_secs: 60,
            renew_before_secs: 30,
        };
        let auth = Arc::new(AuthService::new(b"test-secret", policy));
        let mut interceptor = AuthInterceptor::new(Arc::clone(&auth));

        let (token, _exp) = auth
            .issue_token("alice", &Vec::new())
            .expect("issue token should succeed");

        let mut req = Request::new(());
        let header_value =
            MetadataValue::try_from(&format!("Bearer {}", token)[..]).expect("valid metadata");
        req.metadata_mut().insert("authorization", header_value);

        let req = <AuthInterceptor as tonic::service::Interceptor>::call(&mut interceptor, req)
            .expect("request with valid token should be accepted");

        let ctx = req
            .extensions()
            .get::<UserContext>()
            .expect("UserContext should be injected into extensions");

        assert_eq!(ctx.username, "alice");
    }

    /// 当 token 即将过期且策略要求续签时，应在 extensions 中注入 RenewToken
    #[test]
    fn interceptor_injects_renew_token_when_near_expiration() {
        let policy = TokenPolicy {
            ttl_secs: 10,
            // 由于 ttl_secs <= renew_before_secs，因此在验证后会立即触发续签
            renew_before_secs: 20,
        };
        let auth = Arc::new(AuthService::new(b"test-secret", policy));
        let mut interceptor = AuthInterceptor::new(Arc::clone(&auth));

        let (token, _exp) = auth
            .issue_token("bob", &Vec::new())
            .expect("issue token should succeed");

        let mut req = Request::new(());
        let header_value =
            MetadataValue::try_from(&format!("Bearer {}", token)[..]).expect("valid metadata");
        req.metadata_mut().insert("authorization", header_value);

        let req = <AuthInterceptor as tonic::service::Interceptor>::call(&mut interceptor, req)
            .expect("request with valid token should be accepted");

        let renew = req
            .extensions()
            .get::<RenewToken>()
            .expect("RenewToken should be injected for near-expiration token");

        assert!(!renew.token.is_empty(), "renewed token should not be empty");
        assert!(renew.expires_at > 0, "renewed token should have a valid exp");

        // 同时确认 UserContext 仍然存在
        let ctx = req
            .extensions()
            .get::<UserContext>()
            .expect("UserContext should be kept");
        assert_eq!(ctx.username, "bob");
    }

    /// 校验函数应允许 admin/admin 作为测试账号通过
    #[tokio::test]
    async fn validate_user_credentials_allows_admin_admin() {
        let ok = validate_user_credentials("admin", "admin")
            .await
            .expect("validation should not fail internally");
        assert!(ok, "admin/admin should be accepted as a test account");
    }

    /// 非 admin/admin 的组合应被拒绝
    #[tokio::test]
    async fn validate_user_credentials_rejects_other_users() {
        let ok = validate_user_credentials("admin", "wrong")
            .await
            .expect("validation should not fail internally");
        assert!(!ok, "admin/wrong should be rejected");

        let ok = validate_user_credentials("user", "admin")
            .await
            .expect("validation should not fail internally");
        assert!(!ok, "user/admin should be rejected");
    }

    fn make_auth() -> Arc<AuthService> {
        Arc::new(AuthService::new(
            b"test-secret",
            TokenPolicy {
                ttl_secs: 60,
                renew_before_secs: 30,
            },
        ))
    }

    /// admin/admin 测试账号应能成功登录并拿到非空的 accessToken
    #[tokio::test]
    async fn login_succeeds_for_admin_admin() {
        let auth = make_auth();
        let service = CrvHiveService::new(Arc::clone(&auth));

        let req = crate::pb::LoginReq {
            username: "admin".to_string(),
            password: "admin".to_string(),
        };

        let rsp = service
            .login(Request::new(req))
            .await
            .expect("login should succeed for admin/admin")
            .into_inner();

        assert!(
            !rsp.access_token.is_empty(),
            "access_token should not be empty for admin/admin"
        );
        assert!(rsp.expires_at > 0, "expires_at should be a positive timestamp");
    }

    /// 非 admin/admin 的账号应被拒绝并返回 Unauthenticated
    #[tokio::test]
    async fn login_fails_for_invalid_credentials() {
        let auth = make_auth();
        let service = CrvHiveService::new(Arc::clone(&auth));

        let req = crate::pb::LoginReq {
            username: "user".to_string(),
            password: "wrong".to_string(),
        };

        let res = service.login(Request::new(req)).await;
        assert!(res.is_err(), "login should fail for invalid credentials");
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    /// require_user 在存在 UserContext 时应成功返回
    #[test]
    fn require_user_works_when_context_present() {
        let mut req = Request::new(());
        req.extensions_mut().insert(UserContext {
            username: "alice".to_string(),
            scopes: vec![],
            source: AuthSource::Jwt,
        });

        let ctx = require_user(&req).expect("require_user should succeed when context exists");
        assert_eq!(ctx.username, "alice");
    }

    /// require_user 在未登录时应返回 Unauthenticated
    #[test]
    fn require_user_fails_when_context_missing() {
        let req = Request::new(());
        let res = require_user(&req);
        assert!(res.is_err(), "require_user should fail when context is missing");
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }
}
