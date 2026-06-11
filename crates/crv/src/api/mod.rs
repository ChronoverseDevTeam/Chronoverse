use crv_shared::error::{CrvError, Result};
use reqwest::Client;
use serde_json::Value;

pub struct CrvApiClient { pub base_url: String, pub ticket: Option<String>, http: Client }

impl CrvApiClient {
    pub fn new(base_url: String, ticket: Option<String>) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_string(), ticket, http: Client::new() }
    }
    pub fn url(&self, p: &str) -> String { format!("{}/api/v1{}", self.base_url, p) }
    fn auth(&self) -> Option<String> { self.ticket.as_ref().map(|t| format!("Ticket {t}")) }

    async fn check(&self, resp: reqwest::Response, _path: &str) -> Result<Value> {
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or_default();
        if status.is_success() { Ok(body) }
        else { Err(CrvError::Network(format!("{}: {}", status, body["error"].as_str().unwrap_or("unknown")))) }
    }

    pub async fn get_json(&self, path: &str) -> Result<Value> {
        let mut r = self.http.get(self.url(path));
        if let Some(h) = self.auth() { r = r.header("Authorization", h); }
        self.check(r.send().await.map_err(|e| CrvError::Network(e.to_string()))?, path).await
    }
    pub async fn post_json(&self, path: &str, data: &Value) -> Result<Value> {
        let mut r = self.http.post(self.url(path)).json(data);
        if let Some(h) = self.auth() { r = r.header("Authorization", h); }
        self.check(r.send().await.map_err(|e| CrvError::Network(e.to_string()))?, path).await
    }
    pub async fn post_bytes(&self, path: &str, data: &[u8]) -> Result<Value> {
        let mut r = self.http.post(self.url(path)).body(data.to_vec());
        if let Some(h) = self.auth() { r = r.header("Authorization", h); }
        self.check(r.send().await.map_err(|e| CrvError::Network(e.to_string()))?, path).await
    }
    pub async fn download_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let mut r = self.http.get(self.url(path));
        if let Some(h) = self.auth() { r = r.header("Authorization", h); }
        let resp = r.send().await.map_err(|e| CrvError::Network(e.to_string()))?;
        if resp.status().is_success() { resp.bytes().await.map(|b| b.to_vec()).map_err(|e| CrvError::Network(e.to_string())) }
        else { Err(CrvError::Network(format!("download: {}", resp.status()))) }
    }

    // ── Auth ───────────────────────────────────────
    pub async fn login(&self, u: &str, p: &str) -> Result<(String, String)> {
        let b = self.post_json("/auth/login", &serde_json::json!({"user":u,"password":p})).await?;
        Ok((b["data"]["ticket"].as_str().unwrap_or("").into(), b["data"]["user"].as_str().unwrap_or("").into()))
    }
    pub async fn logout(&self) -> Result<Value> { self.post_json("/auth/logout", &serde_json::json!({})).await }
    pub async fn whoami(&self) -> Result<Value> { self.get_json("/auth/whoami").await }
    pub async fn server_info(&self) -> Result<Value> { self.get_json("/info").await }

    // ── Users ──────────────────────────────────────
    pub async fn list_users(&self) -> Result<Value> { self.get_json("/users").await }
    pub async fn create_user(&self, n: &str, e: &str, pw: &str) -> Result<Value> { self.post_json("/users", &serde_json::json!({"name":n,"email":e,"password":pw})).await }
    pub async fn delete_user(&self, id: &str) -> Result<Value> { self.post_json(&format!("/users/{id}"), &serde_json::json!({})).await }

    // ── Groups ─────────────────────────────────────
    pub async fn list_groups(&self) -> Result<Value> { self.get_json("/groups").await }
    pub async fn create_group(&self, n: &str) -> Result<Value> { self.post_json("/groups", &serde_json::json!({"name":n})).await }
    pub async fn delete_group(&self, n: &str) -> Result<Value> { self.post_json(&format!("/groups/{n}"), &serde_json::json!({})).await }

    // ── Clients ────────────────────────────────────
    pub async fn list_clients(&self) -> Result<Value> { self.get_json("/clients").await }
    pub async fn create_client(&self, n: &str, root: &str) -> Result<Value> { self.post_json("/clients", &serde_json::json!({"name":n,"root":root})).await }
    pub async fn get_client(&self, n: &str) -> Result<Value> { self.get_json(&format!("/clients/{n}")).await }
    pub async fn delete_client(&self, n: &str) -> Result<Value> { self.post_json(&format!("/clients/{n}"), &serde_json::json!({})).await }

    // ── Files ──────────────────────────────────────
    pub async fn open_files(&self, c: &str, a: &str, f: &[String]) -> Result<Value> { self.post_json(&format!("/clients/{c}/files/{a}"), &serde_json::json!({"files":f})).await }
    pub async fn revert_files(&self, c: &str, f: &[String]) -> Result<Value> { self.post_json(&format!("/clients/{c}/files/revert"), &serde_json::json!({"files":f})).await }
    pub async fn list_opened(&self, c: &str) -> Result<Value> { self.get_json(&format!("/clients/{c}/files/opened")).await }

    // ── Sync ───────────────────────────────────────
    pub async fn sync(&self, c: &str, spec: &str, force: bool) -> Result<Value> {
        let p = if spec.is_empty() { format!("/clients/{c}/sync?force={force}") } else { format!("/clients/{c}/sync?filespec={spec}&force={force}") };
        self.get_json(&p).await
    }
    pub async fn confirm_sync(&self, c: &str, e: &[Value]) -> Result<Value> { self.post_json(&format!("/clients/{c}/sync/confirm"), &serde_json::json!(e)).await }

    // ── Changes ────────────────────────────────────
    pub async fn list_changes(&self, c: &str) -> Result<Value> { self.get_json(&format!("/clients/{c}/changes")).await }
    pub async fn create_change(&self, c: &str, d: &str) -> Result<Value> { self.post_json(&format!("/clients/{c}/changes"), &serde_json::json!({"description":d})).await }
    pub async fn submit_change(&self, c: &str, cid: &str) -> Result<Value> { self.post_json(&format!("/clients/{c}/changes/{cid}/submit"), &serde_json::json!({})).await }

    // ── Content ────────────────────────────────────
    pub async fn upload_content(&self, p: &str, d: &[u8]) -> Result<Value> { self.post_bytes(&format!("/files/content/{p}"), d).await }
    pub async fn download_content(&self, p: &str, rev: Option<i32>) -> Result<Vec<u8>> {
        let u = if let Some(r) = rev { format!("/files/content/{p}?rev={r}") } else { format!("/files/content/{p}") };
        self.download_bytes(&u).await
    }
    pub async fn file_stat(&self, p: &str) -> Result<Value> { self.get_json(&format!("/files/fstat/{p}")).await }
    pub async fn file_log(&self, p: &str, max: i64) -> Result<Value> { self.get_json(&format!("/files/filelog/{p}?max={max}")).await }

    // ── Locks ──────────────────────────────────────
    pub async fn lock_files(&self, c: &str, f: &[String]) -> Result<Value> { self.post_json(&format!("/clients/{c}/files/lock"), &serde_json::json!({"files":f})).await }
    pub async fn unlock_files(&self, f: &[String]) -> Result<Value> { self.post_json("/files/unlock", &serde_json::json!({"files":f})).await }
    pub async fn list_locks(&self) -> Result<Value> { self.get_json("/files/locks").await }

    // ── Branch ────────────────────────────────────
    pub async fn list_branches(&self) -> Result<Value> { self.get_json("/branches").await }
    pub async fn create_branch(&self, n: &str, d: &str) -> Result<Value> { self.post_json("/branches", &serde_json::json!({"name":n,"description":d})).await }
    pub async fn delete_branch(&self, n: &str) -> Result<Value> { self.post_json(&format!("/branches/{n}"), &serde_json::json!({})).await }

    pub async fn list_streams(&self) -> Result<Value> { self.get_json("/streams").await }
    pub async fn create_stream(&self, n: &str, p: Option<&str>, t: &str) -> Result<Value> { self.post_json("/streams", &serde_json::json!({"name":n,"parent_id":p,"stream_type":t})).await }
    pub async fn delete_stream(&self, n: &str) -> Result<Value> { self.post_json(&format!("/streams/{n}"), &serde_json::json!({})).await }
    pub async fn list_protections(&self) -> Result<Value> { self.get_json("/protections").await }
    pub async fn add_protection(&self, pt: &str, pl: &str, et: &str, en: &str, dp: &str) -> Result<Value> {
        self.post_json("/protections", &serde_json::json!({"perm_type":pt,"perm_level":pl,"entity_type":et,"entity_name":en,"depot_path_pattern":dp})).await
    }
    pub async fn delete_protection(&self, id: &str) -> Result<Value> { self.post_json(&format!("/protections/{id}"), &serde_json::json!({})).await }

    // ── Integrate ──────────────────────────────────
    pub async fn integrate(&self, source: &str, target: &str, action: &str) -> Result<Value> {
        self.post_json("/integrate", &serde_json::json!({"source":source,"target":target,"action":action})).await
    }
    pub async fn list_integrations(&self, path: Option<&str>) -> Result<Value> {
        let p = if let Some(pa) = path { format!("/integrations?path={pa}") } else { "/integrations".into() };
        self.get_json(&p).await
    }

    // ── Labels ─────────────────────────────────────
    pub async fn list_labels(&self) -> Result<Value> { self.get_json("/labels").await }
    pub async fn create_label(&self, n: &str, d: &str) -> Result<Value> { self.post_json("/labels", &serde_json::json!({"name":n,"description":d})).await }
    pub async fn delete_label(&self, n: &str) -> Result<Value> { self.post_json(&format!("/labels/{n}"), &serde_json::json!({})).await }
    pub async fn label_sync(&self, name: &str, filespec: &str) -> Result<Value> {
        self.post_json(&format!("/labels/{name}/sync"), &serde_json::json!({"filespec":filespec})).await
    }
    pub async fn label_revisions(&self, name: &str) -> Result<Value> { self.get_json(&format!("/labels/{name}/revisions")).await }
    pub async fn label_clear(&self, name: &str) -> Result<Value> { self.post_json(&format!("/labels/{name}/clear"), &serde_json::json!({})).await }
}
