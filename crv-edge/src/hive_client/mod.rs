use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::Channel;

use crate::hive_pb::hive_service_client::HiveServiceClient;
use crate::hive_pb::*;

pub struct AuthState {
    access_token: Option<String>,
    persist_path: Option<PathBuf>,
}

impl AuthState {
    fn new() -> Self {
        Self {
            access_token: None,
            persist_path: None,
        }
    }
}

#[derive(Clone)]
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
    inner: HiveServiceClient<tonic::codegen::InterceptedService<Channel, AuthInterceptor>>,
    state: Arc<Mutex<AuthState>>,
}

impl HiveClient {
    pub async fn connect<D: Into<String>>(dst: D) -> Result<Self, tonic::Status> {
        let channel = Channel::from_shared(dst.into())
            .map_err(|_| tonic::Status::invalid_argument("invalid uri"))?
            .connect()
            .await
            .map_err(|e| tonic::Status::unavailable(format!("connect failed: {}", e)))?;
        let state = Arc::new(Mutex::new(AuthState::new()));
        let interceptor = AuthInterceptor {
            state: state.clone(),
        };
        let inner = HiveServiceClient::with_interceptor(channel, interceptor);
        Ok(Self { inner, state })
    }

    /// 使用已有的 Channel 创建 HiveClient（用于连接池）
    pub fn from_channel(channel: Channel) -> Self {
        let state = Arc::new(Mutex::new(AuthState::new()));
        let interceptor = AuthInterceptor {
            state: state.clone(),
        };
        let inner = HiveServiceClient::with_interceptor(channel, interceptor);
        Self { inner, state }
    }

    pub fn set_token<S: Into<String>>(&self, token: S) {
        let token_str: String = token.into();
        let persist_path: Option<PathBuf> = {
            let mut guard = self.state.lock().unwrap();
            guard.access_token = Some(token_str.clone());
            guard.persist_path.clone()
        };
        if let Some(path) = persist_path {
            let t = token_str.clone();
            tokio::spawn(async move {
                let _ = persist_token(&path, &t).await;
            });
        }
    }

    pub fn clear_token(&self) {
        self.state.lock().unwrap().access_token = None;
    }

    pub fn get_token(&self) -> Option<String> {
        self.state.lock().unwrap().access_token.clone()
    }

    /// 设置 JWT 的持久化路径；传入 None 将关闭持久化
    pub fn set_token_persist_path<P: Into<PathBuf>>(&self, path: P) {
        self.state.lock().unwrap().persist_path = Some(path.into());
    }

    /// 使用默认路径（用户目录下的 .crv/jwt）
    pub fn set_default_token_persist_path(&self) {
        self.state.lock().unwrap().persist_path = Some(default_jwt_path());
    }

    // Hive Service Methods
    pub async fn bonjour(&mut self) -> Result<BonjourRsp, tonic::Status> {
        let req = BonjourReq {};
        let rsp: tonic::Response<BonjourRsp> = self.inner.bonjour(req).await?;
        Ok(rsp.into_inner())
    }
}

impl Clone for HiveClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            state: self.state.clone(),
        }
    }
}

fn default_jwt_path() -> PathBuf {
    if let Some(user_dirs) = directories::UserDirs::new() {
        let home = user_dirs.home_dir();
        return home.join(".crv").join("jwt");
    }

    Path::new(".").join(".crv").join("jwt")
}

async fn persist_token(path: &Path, token: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent).await;
    }
    fs::write(path, token).await
}

impl HiveClient {
    /// 从磁盘读取 JWT 并设置（若存在）
    pub async fn load_token_from_disk<P: Into<PathBuf>>(
        &self,
        path: Option<P>,
    ) -> Result<bool, io::Error> {
        let path = match path {
            Some(p) => p.into(),
            None => default_jwt_path(),
        };
        if let Ok(data) = fs::read_to_string(&path).await {
            let token = data.trim().to_string();
            if !token.is_empty() {
                self.set_token(token);
                return Ok(true);
            }
        }
        Ok(false)
    }
}
