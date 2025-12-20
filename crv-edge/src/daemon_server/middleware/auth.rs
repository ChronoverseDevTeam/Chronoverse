use crate::daemon_server::{config::RuntimeConfig, context::SessionContext, state::AppState};
use tonic::{Request, Status};

pub fn call(_state: AppState, mut request: Request<()>) -> Result<Request<()>, Status> {
    // 1. 从 runtime config 中读出 user
    let config = request
        .extensions()
        .get::<RuntimeConfig>()
        .expect("Can't get read runtime config.");
    let username = config.user.clone();

    let token_str = format!("token-{}", username.value); // todo: 根据用户名从 db 中获取当前用户的 token

    let context = SessionContext {
        username: username.value,
        token: token_str,
    };

    // 2. 将解析出的会话上下文注入到 request context 中
    // 这样后续的 Handler 可以通过 request.extensions() 获取
    request.extensions_mut().insert(context);

    Ok(request)
}
