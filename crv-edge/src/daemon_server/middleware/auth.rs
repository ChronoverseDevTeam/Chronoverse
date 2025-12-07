use crate::daemon_server::context::SessionContext;
use tonic::{Request, Status};

pub fn call(mut request: Request<()>) -> Result<Request<()>, Status> {
    // 1. 获取用户名
    let username = request.metadata().get("x-crv-username").cloned();

    match username {
        Some(username) => {
            let username = username
                .to_str()
                .map_err(|e| Status::internal(e.to_string()))?;
            let token_str = format!("token-{}", username); // todo: 根据用户名从 db 中获取当前用户的 token

            let context = SessionContext {
                username: username.to_string(),
                token: token_str,
            };

            // 2. 将解析出的会话上下文注入到 request context 中
            // 这样后续的 Handler 可以通过 request.extensions() 获取
            request.extensions_mut().insert(context);

            Ok(request)
        }
        None => {
            // 尝试从 db 中获取默认用户
            todo!()
        }
    }
}
