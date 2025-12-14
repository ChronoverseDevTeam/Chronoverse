use crate::daemon_server::{config::RuntimeConfig, context::SessionContext, state::AppState};
use tonic::{Request, Status};

pub fn call(state: AppState, mut request: Request<()>) -> Result<Request<()>, Status> {
    // 1. 获取用户名
    let username = request.metadata().get("x-crv-username").cloned();

    let username = match username {
        Some(username) => username
            .to_str()
            .map_err(|e| Status::internal(e.to_string()))?
            .to_string(),
        None => {
            // 从配置中读取默认用户
            let config = request
                .extensions()
                .get::<RuntimeConfig>()
                .expect("Can't get read runtime config.");
            config.default_user.clone()
        }
    };

    let token_str = format!("token-{}", username); // todo: 根据用户名从 db 中获取当前用户的 token

    let context = SessionContext {
        username,
        token: token_str,
    };

    // 2. 将解析出的会话上下文注入到 request context 中
    // 这样后续的 Handler 可以通过 request.extensions() 获取
    request.extensions_mut().insert(context);

    Ok(request)
}
