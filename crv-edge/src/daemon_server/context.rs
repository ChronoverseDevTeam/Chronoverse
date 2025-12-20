//! 提取请求上下文，包括会话上下文、配置上下文等。
//! 这些上下文由中间件注入。
use tonic::Request;
use tonic::Status;

use crate::daemon_server::config::RuntimeConfig;

use super::error::AppResult;

/// 会话上下文，包括用户名和令牌。
#[derive(Clone)]
pub struct SessionContext {
    pub username: String,
    pub token: String,
}

impl SessionContext {
    pub fn from_req<T>(req: &Request<T>) -> AppResult<Self> {
        let context = req
            .extensions()
            .get::<Self>() // AuthInterceptor 放进去的是 Self
            .ok_or(Status::unauthenticated("Missing user context"))?
            .clone();

        Ok(context.clone())
    }
}

impl RuntimeConfig {
    pub fn from_req<T>(req: &Request<T>) -> AppResult<Self> {
        let context = req
            .extensions()
            .get::<Self>() // AuthInterceptor 放进去的是 Self
            .ok_or(Status::unauthenticated("Missing runtime config"))?
            .clone();

        Ok(context.clone())
    }
}
