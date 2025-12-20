// use std::io;
// use std::path::{Path, PathBuf};
// use std::sync::{Arc, Mutex};
// use tokio::fs;
// use tokio::task::JoinHandle;
// use tonic::codegen::InterceptedService;
// use tonic::metadata::MetadataValue;
// use tonic::service::Interceptor;
// use tonic::transport::Channel;

// use crate::hive_pb::hive_service_client::HiveServiceClient;
// use crate::hive_pb::*;

// pub struct AuthState {
//     access_token: Option<String>,
//     persist_path: Option<PathBuf>,
// }

// impl AuthState {
//     fn new() -> Self {
//         Self {
//             access_token: None,
//             persist_path: None,
//         }
//     }
// }

// #[derive(Clone)]
// struct AuthInterceptor {
//     state: Arc<Mutex<AuthState>>,
// }

// impl Interceptor for AuthInterceptor {
//     fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
//         if let Some(token) = self.state.lock().unwrap().access_token.clone() {
//             let header_val = format!("Bearer {}", token);
//             if let Ok(v) = MetadataValue::try_from(header_val.as_str()) {
//                 let _ = req.metadata_mut().insert("authorization", v);
//             }
//         }
//         Ok(req)
//     }
// }

// pub struct HiveClient {
//     inner: HiveServiceClient<InterceptedService<Channel, AuthInterceptor>>,
//     state: Arc<Mutex<AuthState>>,
//     heartbeat_handle: Option<JoinHandle<()>>,
// }

// impl HiveClient {
//     pub async fn connect<D: Into<String>>(dst: D) -> Result<Self, tonic::Status> {
//         let channel = Channel::from_shared(dst.into())
//             .map_err(|_| tonic::Status::invalid_argument("invalid uri"))?
//             .connect()
//             .await
//             .map_err(|e| tonic::Status::unavailable(format!("connect failed: {}", e)))?;
//         let state = Arc::new(Mutex::new(AuthState::new()));
//         let interceptor = AuthInterceptor {
//             state: state.clone(),
//         };
//         let inner = HiveServiceClient::with_interceptor(channel, interceptor);
//         Ok(Self {
//             inner,
//             state,
//             heartbeat_handle: None,
//         })
//     }

//     pub fn set_token<S: Into<String>>(&self, token: S) {
//         let token_str: String = token.into();
//         let persist_path: Option<PathBuf> = {
//             let mut guard = self.state.lock().unwrap();
//             guard.access_token = Some(token_str.clone());
//             guard.persist_path.clone()
//         };
//         if let Some(path) = persist_path {
//             let t = token_str.clone();
//             tokio::spawn(async move {
//                 let _ = persist_token(&path, &t).await;
//             });
//         }
//     }
//     pub fn clear_token(&self) {
//         self.state.lock().unwrap().access_token = None;
//     }

//     pub fn get_token(&self) -> Option<String> {
//         self.state.lock().unwrap().access_token.clone()
//     }

//     /// 设置 JWT 的持久化路径；传入 None 将关闭持久化
//     pub fn set_token_persist_path<P: Into<PathBuf>>(&self, path: P) {
//         self.state.lock().unwrap().persist_path = Some(path.into());
//     }

//     /// 使用默认路径（用户目录下的 .crv/jwt）
//     pub fn set_default_token_persist_path(&self) {
//         self.state.lock().unwrap().persist_path = Some(default_jwt_path());
//     }

//     // Auth
//     pub async fn login(
//         &mut self,
//         username: String,
//         password: String,
//     ) -> Result<LoginRsp, tonic::Status> {
//         let req = LoginReq { username, password };
//         let rsp = self.inner.login(req).await?;
//         let rsp = rsp.into_inner();
//         self.set_token(rsp.access_token.clone());
//         Ok(rsp)
//     }

//     pub async fn register(
//         &mut self,
//         username: String,
//         password: String,
//         email: String,
//     ) -> Result<RegisterRsp, tonic::Status> {
//         let req = RegisterReq {
//             username,
//             password,
//             email,
//         };
//         let rsp = self.inner.register(req).await?;
//         Ok(rsp.into_inner())
//     }

//     // Workspace
//     pub async fn list_workspaces(
//         &mut self,
//         name: Option<String>,
//         owner: Option<String>,
//         device_finger_print: Option<String>,
//     ) -> Result<ListWorkspaceRsp, tonic::Status> {
//         let req = ListWorkspaceReq {
//             name,
//             owner,
//             device_finger_print,
//         };
//         let rsp = self.inner.list_workspaces(req).await?;
//         self.maybe_update_token_from_metadata(rsp.metadata());
//         Ok(rsp.into_inner())
//     }

//     pub async fn upsert_workspace(
//         &mut self,
//         name: String,
//         path: String,
//         device_finger_print: String,
//     ) -> Result<NilRsp, tonic::Status> {
//         let req = UpsertWorkspaceReq {
//             name,
//             path,
//             device_finger_print,
//         };
//         let rsp = self.inner.upsert_workspace(req).await?;
//         self.maybe_update_token_from_metadata(rsp.metadata());
//         Ok(rsp.into_inner())
//     }

//     // Misc
//     pub async fn greeting(&mut self, msg: String) -> Result<NilRsp, tonic::Status> {
//         let req = GreetingReq { msg };
//         let rsp = self.inner.greeting(req).await?;
//         self.maybe_update_token_from_metadata(rsp.metadata());
//         Ok(rsp.into_inner())
//     }

//     // Tokens
//     fn maybe_update_token_from_metadata(&self, md: &tonic::metadata::MetadataMap) {
//         if let Some(v) = md.get("x-renew-token").and_then(|v| v.to_str().ok()) {
//             self.set_token(v.to_string());
//         }
//     }
// }

// impl Clone for HiveClient {
//     fn clone(&self) -> Self {
//         Self {
//             inner: self.inner.clone(),
//             state: self.state.clone(),
//             heartbeat_handle: None,
//         }
//     }
// }

// fn default_jwt_path() -> PathBuf {
//     if let Some(user_dirs) = directories::UserDirs::new() {
//         let home = user_dirs.home_dir();
//         return home.join(".crv").join("jwt");
//     }

//     Path::new(".").join(".crv").join("jwt")
// }

// async fn persist_token(path: &Path, token: &str) -> Result<(), std::io::Error> {
//     if let Some(parent) = path.parent() {
//         let _ = fs::create_dir_all(parent).await;
//     }
//     fs::write(path, token).await
// }

// impl HiveClient {
//     /// 从磁盘读取 JWT 并设置（若存在）
//     pub async fn load_token_from_disk<P: Into<PathBuf>>(
//         &self,
//         path: Option<P>,
//     ) -> Result<bool, io::Error> {
//         let path = match path {
//             Some(p) => p.into(),
//             None => default_jwt_path(),
//         };
//         if let Ok(data) = fs::read_to_string(&path).await {
//             let token = data.trim().to_string();
//             if !token.is_empty() {
//                 self.set_token(token);
//                 return Ok(true);
//             }
//         }
//         Ok(false)
//     }

//     /// 启动心跳：固定间隔调用 Greeting，自动续签时会同步落盘
//     pub fn start_heartbeat(&mut self, interval: std::time::Duration, msg: String) {
//         if let Some(h) = self.heartbeat_handle.take() {
//             h.abort();
//         }
//         let mut client = self.clone();
//         let handle = tokio::spawn(async move {
//             let mut ticker = tokio::time::interval(interval);
//             loop {
//                 ticker.tick().await;
//                 let _ = client.greeting(msg.clone()).await;
//             }
//         });
//         self.heartbeat_handle = Some(handle);
//     }

//     /// 停止心跳
//     pub fn stop_heartbeat(&mut self) {
//         if let Some(h) = self.heartbeat_handle.take() {
//             h.abort();
//         }
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[tokio::test]
//     async fn test_register_user() -> Result<(), Box<dyn std::error::Error>> {
//         if std::env::var("GITHUB_ACTIONS").is_ok() || std::env::var("CI").is_ok() {
//             eprintln!("Skip test_register_user on GitHub Actions CI");
//             return Ok(());
//         }
//         let endpoint = std::env::var("CRV_HIVE_ENDPOINT")
//             .unwrap_or_else(|_| "http://127.0.0.1:34560".to_string());
//         let mut client = HiveClient::connect(endpoint).await?;
//         let username = "Alice".to_string();
//         let _ = client
//             .register(username, "pw".into(), "b@c.d".into())
//             .await?;
//         Ok(())
//     }

//     #[tokio::test]
//     async fn test_login_and_create_workspace() -> Result<(), Box<dyn std::error::Error>> {
//         if std::env::var("GITHUB_ACTIONS").is_ok() || std::env::var("CI").is_ok() {
//             eprintln!("Skip test_login_and_create_workspace on GitHub Actions CI");
//             return Ok(());
//         }
//         let endpoint = std::env::var("CRV_HIVE_ENDPOINT")
//             .unwrap_or_else(|_| "http://127.0.0.1:34560".to_string());
//         let mut client = HiveClient::connect(endpoint).await?;
//         let ts = chrono::Utc::now().timestamp_millis();
//         let username = "Alice".to_string();
//         let ws = format!("w_{}", ts);
//         let _ = client
//             .register(username.clone(), "pw".into(), "b@c.d".into())
//             .await
//             .ok();
//         let _ = client.login(username, "pw".into()).await?;
//         let _ = client
//             .upsert_workspace(ws, "C:/tmp/w1".into(), "fp123".into())
//             .await?;
//         Ok(())
//     }
// }
