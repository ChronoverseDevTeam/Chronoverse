use std::sync::{Arc, Mutex};
use tonic::codegen::InterceptedService;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::Channel;

use crate::hive_pb::hive_service_client::HiveServiceClient;
use crate::hive_pb::*;

pub struct AuthState {
    access_token: Option<String>,
}

impl AuthState {
    fn new() -> Self { Self { access_token: None } }
}

struct AuthInterceptor {
    state: Arc<Mutex<AuthState>>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(token) = self.state.lock().unwrap().access_token.clone() {
            let header_val = format!("Bearer {}", token);
            if let Ok(v) = MetadataValue::try_from(header_val.as_str()) {
                let _ = req.metadata_mut().insert("authorization", v);
            }
        }
        Ok(req)
    }
}

pub struct HiveClient {
    inner: HiveServiceClient<InterceptedService<Channel, AuthInterceptor>>,
    state: Arc<Mutex<AuthState>>,
}

impl HiveClient {
    pub async fn connect<D: Into<String>>(dst: D) -> Result<Self, tonic::Status> {
        let channel = Channel::from_shared(dst.into()).map_err(|_| tonic::Status::invalid_argument("invalid uri"))?
            .connect().await.map_err(|e| tonic::Status::unavailable(format!("connect failed: {}", e)))?;
        let state = Arc::new(Mutex::new(AuthState::new()));
        let interceptor = AuthInterceptor { state: state.clone() };
        let inner = HiveServiceClient::with_interceptor(channel, interceptor);
        Ok(Self { inner, state })
    }

    pub fn set_token<S: Into<String>>(&self, token: S) { self.state.lock().unwrap().access_token = Some(token.into()); }
    pub fn clear_token(&self) { self.state.lock().unwrap().access_token = None; }

    // Auth
    pub async fn login(&mut self, username: String, password: String) -> Result<LoginRsp, tonic::Status> {
        let req = LoginReq { username, password };
        let rsp = self.inner.login(req).await?;
        let rsp = rsp.into_inner();
        self.set_token(rsp.access_token.clone());
        Ok(rsp)
    }

    pub async fn register(&mut self, username: String, password: String, email: String) -> Result<RegisterRsp, tonic::Status> {
        let req = RegisterReq { username, password, email };
        let rsp = self.inner.register(req).await?;
        Ok(rsp.into_inner())
    }

    // Workspace
    pub async fn list_workspaces(&mut self, name: Option<String>, owner: Option<String>, device_finger_print: Option<String>) -> Result<ListWorkspaceRsp, tonic::Status> {
        let req = ListWorkspaceReq { name, owner, device_finger_print };
        let rsp = self.inner.list_workspaces(req).await?;
        self.maybe_update_token_from_metadata(rsp.metadata());
        Ok(rsp.into_inner())
    }

    // Tokens
    pub async fn create_token(&mut self, name: String, expires_at: Option<i64>, scopes: Vec<String>) -> Result<CreateTokenRsp, tonic::Status> {
        let req = CreateTokenReq { name, expires_at, scopes };
        let rsp = self.inner.create_token(req).await?;
        self.maybe_update_token_from_metadata(rsp.metadata());
        Ok(rsp.into_inner())
    }

    pub async fn list_tokens(&mut self) -> Result<ListTokensRsp, tonic::Status> {
        let req = ListTokensReq {};
        let rsp = self.inner.list_tokens(req).await?;
        self.maybe_update_token_from_metadata(rsp.metadata());
        Ok(rsp.into_inner())
    }

    pub async fn revoke_token(&mut self, id: String) -> Result<RevokeTokenRsp, tonic::Status> {
        let req = RevokeTokenReq { id };
        let rsp = self.inner.revoke_token(req).await?;
        self.maybe_update_token_from_metadata(rsp.metadata());
        Ok(rsp.into_inner())
    }

    fn maybe_update_token_from_metadata(&self, md: &tonic::metadata::MetadataMap) {
        if let Some(v) = md.get("x-renew-token").and_then(|v| v.to_str().ok()) {
            self.set_token(v.to_string());
        }
    }
}


