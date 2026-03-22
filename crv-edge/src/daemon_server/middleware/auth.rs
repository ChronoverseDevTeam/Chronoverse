use crate::daemon_server::{config::RuntimeConfig, context::SessionContext, state::AppState};
use tonic::{Request, Status};

pub fn call(_state: AppState, mut request: Request<()>) -> Result<Request<()>, Status> {
    // 1. Read user and auth token from runtime config
    let config = request
        .extensions()
        .get::<RuntimeConfig>()
        .expect("Can't get read runtime config.");
    let username = config.user.clone();
    let token = config.auth_token.clone();

    let context = SessionContext {
        username: username.value,
        token: token.value,
    };

    // 2. Inject session context into request extensions
    request.extensions_mut().insert(context);

    Ok(request)
}
