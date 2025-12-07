//! 中间件层
use crate::daemon_server::state::AppState;
use tonic::{Request, Status, service::Interceptor};

pub mod auth;
pub mod config;

#[derive(Clone)]
pub struct CombinedInterceptor {
    state: AppState,
}

impl CombinedInterceptor {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl Interceptor for CombinedInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let request = auth::call(request)?;
        let request = config::call(self.state.clone(), request)?;
        Ok(request)
    }
}
