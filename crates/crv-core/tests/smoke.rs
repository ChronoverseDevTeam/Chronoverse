//! Smoke / integration test suite for crv-core.
//!
//! Run against a live server:
//!   docker compose up -d
//!   CRV_CORE_URL=http://localhost:3000 cargo test --test smoke -- --ignored --nocapture

use serde_json::{json, Value};
use std::env;

fn core_url() -> String {
    env::var("CRV_CORE_URL").unwrap_or_else(|_| "http://localhost:3000".into())
}

struct Client {
    base: String,
    ticket: Option<String>,
    http: reqwest::blocking::Client,
}

impl Client {
    fn new() -> Self {
        Self { base: core_url(), ticket: None, http: reqwest::blocking::Client::new() }
    }

    fn set_ticket(&mut self, t: &str) { self.ticket = Some(t.to_string()); }

    fn auth(&self) -> Option<String> {
        self.ticket.as_ref().map(|t| format!("Ticket {t}"))
    }

    fn get(&self, path: &str) -> Value {
        let url = format!("{}/api/v1{}", self.base, path);
        let mut req = self.http.get(&url);
        if let Some(h) = self.auth() { req = req.header("Authorization", h); }
        let resp = req.send().expect(&format!("GET {path}"));
        let body: Value = resp.json().expect("json parse");
        assert!(body["success"].as_bool().unwrap_or(false), "GET {path} failed: {body}");
        body
    }

fn post_ok(&self, path: &str, data: &Value) -> Value {
    let url = format!("{}/api/v1{}", self.base, path);
    let mut req = self.http.post(&url).json(data);
    if let Some(h) = self.auth() { req = req.header("Authorization", h); }
    let resp = req.send().expect(&format!("POST {path}"));
    let status = resp.status();
    let body: Value = resp.json().expect("json parse");
    if !(body["success"].as_bool().unwrap_or(false) || status == 201) {
        let err = body["error"].as_str().unwrap_or("unknown");
        // Ignore "already exists" errors for idempotency
        if err.contains("already exists") || err.contains("already performed") {
            return body;
        }
        panic!("POST {path} failed ({}): {body}", status);
    }
    body
}

    /// POST without asserting success (for bootstrap, etc.)
    fn post_raw(&self, path: &str, data: &Value) -> Value {
        let url = format!("{}/api/v1{}", self.base, path);
        let mut req = self.http.post(&url).json(data);
        if let Some(h) = self.auth() { req = req.header("Authorization", h); }
        let resp = req.send().expect(&format!("POST {path}"));
        resp.json().expect("json parse")
    }

    fn post_bytes(&self, path: &str, data: &[u8]) -> Value {
        let url = format!("{}/api/v1{}", self.base, path);
        let mut req = self.http.post(&url).body(data.to_vec());
        if let Some(h) = self.auth() { req = req.header("Authorization", h); }
        let resp = req.send().expect(&format!("POST {path}"));
        let status = resp.status();
        let body: Value = resp.json().expect("json parse");
        assert!(body["success"].as_bool().unwrap_or(false) || status == 201,
            "POST bytes {path} failed ({}): {body}", status);
        body
    }

    fn download(&self, path: &str) -> Vec<u8> {
        let url = format!("{}/api/v1{}", self.base, path);
        let mut req = self.http.get(&url);
        if let Some(h) = self.auth() { req = req.header("Authorization", h); }
        let resp = req.send().expect(&format!("DOWNLOAD {path}"));
        assert!(resp.status().is_success(), "DOWNLOAD {path} failed: {}", resp.status());
        resp.bytes().expect("bytes").to_vec()
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[test]
#[ignore = "requires running crv-core server"]
fn smoke_all() {
    let mut c = Client::new();

    // ── 1. Bootstrap ─────────────────────────────────────────────
    println!("=== 1. Bootstrap ===");
    let v = c.post_raw("/bootstrap", &json!({}));
    if v["success"].as_bool() == Some(false) {
        println!("  bootstrap already done, continuing");
    } else {
        println!("  admin created: {}", v["data"]["user"]);
    }

    // ── 2. Login ─────────────────────────────────────────────────
    println!("=== 2. Login ===");
    let v = c.post_ok("/auth/login", &json!({"user":"admin","password":"admin123"}));
    let ticket = v["data"]["ticket"].as_str().unwrap().to_string();
    c.set_ticket(&ticket);
    println!("  ticket: {}...", &ticket[..20]);

    // ── 3. Whoami ────────────────────────────────────────────────
    println!("=== 3. Whoami ===");
    let v = c.get("/auth/whoami");
    assert_eq!(v["data"]["user_name"], "admin");

    // ── 4. Create user ───────────────────────────────────────────
    println!("=== 4. Create User ===");
    c.post_ok("/users", &json!({"name":"dev1","email":"dev1@test.com","password":"pass123"}));
    println!("  user dev1 created");

    // ── 5. List users ────────────────────────────────────────────
    println!("=== 5. List Users ===");
    let v = c.get("/users");
    let users = v["data"].as_array().unwrap();
    assert!(users.len() >= 2, "expected >=2 users");
    println!("  {} users", users.len());

    // ── 6. Create group ──────────────────────────────────────────
    println!("=== 6. Create Group ===");
    c.post_ok("/groups", &json!({"name":"devs"}));
    println!("  group devs created");

    // ── 7. List groups ───────────────────────────────────────────
    println!("=== 7. List Groups ===");
    let v = c.get("/groups");
    assert!(v["data"].as_array().unwrap().len() >= 1);

    // ── 8. Create client ─────────────────────────────────────────
    println!("=== 8. Create Client ===");
    c.post_ok("/clients", &json!({"name":"smoke-ws","root":"/tmp/ws"}));
    println!("  client smoke-ws created");

    // ── 9. Open files for add ────────────────────────────────────
    println!("=== 9. Open for Add ===");
    c.post_ok("/clients/smoke-ws/files/add", &json!({"files":["//depot/main/hello.txt","//depot/main/src/lib.rs"]}));
    println!("  2 files opened for add");

    // ──10. List opened files ─────────────────────────────────────
    println!("=== 10. Opened Files ===");
    let v = c.get("/clients/smoke-ws/files/opened");
    assert_eq!(v["data"].as_array().unwrap().len(), 2);
    println!("  2 files opened");

    // ──11. Upload content ────────────────────────────────────────
    println!("=== 11. Upload Content ===");
    c.post_bytes("/files/content//depot/main/hello.txt", b"Hello Chronoverse!");
    c.post_bytes("/files/content//depot/main/src/lib.rs", b"pub fn main() {}");
    println!("  content uploaded");

    // ──12. Create changelist ─────────────────────────────────────
    println!("=== 12. Create Changelist ===");
    let v = c.post_ok("/clients/smoke-ws/changes", &json!({"description":"smoke test init"}));
    let change_id = v["data"]["id"].as_str().unwrap().to_string();
    println!("  changelist: {change_id}");

    // ──13. Submit ────────────────────────────────────────────────
    println!("=== 13. Submit ===");
    let v = c.post_ok(&format!("/clients/smoke-ws/changes/{change_id}/submit"), &json!({}));
    println!("  submitted: change {} ({} files)",
        v["data"]["change_number"], v["data"]["files_submitted"]);

    // ──14. List changes ──────────────────────────────────────────
    println!("=== 14. Changes ===");
    let v = c.get("/clients/smoke-ws/changes");
    assert!(v["data"].as_array().unwrap().len() >= 1);

    // ──15. File stat ─────────────────────────────────────────────
    println!("=== 15. Fstat ===");
    let v = c.get("/files/fstat//depot/main/hello.txt");
    assert_eq!(v["data"]["head_revision"], 1);
    println!("  hello.txt rev 1 ok");

    // ──16. File log ──────────────────────────────────────────────
    println!("=== 16. Filelog ===");
    let v = c.get("/files/filelog//depot/main/hello.txt?max=10");
    assert_eq!(v["data"].as_array().unwrap().len(), 1);

    // ──17. Download content ──────────────────────────────────────
    println!("=== 17. Download ===");
    let content = c.download("/files/content//depot/main/hello.txt?rev=1");
    assert_eq!(String::from_utf8_lossy(&content), "Hello Chronoverse!");
    println!("  content verified");

    // ──18. Sync ──────────────────────────────────────────────────
    println!("=== 18. Sync ===");
    let v = c.get("/clients/smoke-ws/sync?force=true");
    let files = v["data"]["files"].as_array().unwrap();
    println!("  sync: {} files available", files.len());

    // ──19. Lock file ─────────────────────────────────────────────
    println!("=== 19. Lock ===");
    c.post_ok("/clients/smoke-ws/files/lock", &json!({"files":["//depot/main/hello.txt"]}));
    println!("  hello.txt locked");

    // ──20. List locks ────────────────────────────────────────────
    println!("=== 20. List Locks ===");
    let v = c.get("/files/locks");
    assert!(v["data"].as_array().unwrap().len() >= 1);

    // ──21. Unlock ────────────────────────────────────────────────
    println!("=== 21. Unlock ===");
    c.post_ok("/files/unlock", &json!({"files":["//depot/main/hello.txt"]}));
    println!("  unlocked");

    // ──22. Branch spec ───────────────────────────────────────────
    println!("=== 22. Branch ===");
    c.post_ok("/branches", &json!({"name":"main-dev","description":"main to dev"}));
    println!("  branch main-dev created");

    // ──23. List branches ─────────────────────────────────────────
    println!("=== 23. List Branches ===");
    c.get("/branches");

    // ──24. Integrate ─────────────────────────────────────────────
    println!("=== 24. Integrate ===");
    let v = c.post_ok("/integrate", &json!({
        "source":"//depot/main/...","target":"//depot/dev/...","action":"branch_from"
    }));
    println!("  {} files branched", v["data"]["files_branched"]);

    // ──25. Label ─────────────────────────────────────────────────
    println!("=== 25. Label ===");
    c.post_ok("/labels", &json!({"name":"v1.0"}));
    println!("  label v1.0 created");

    // ──26. Label sync ────────────────────────────────────────────
    println!("=== 26. Label Sync ===");
    let v = c.post_ok("/labels/v1.0/sync", &json!({"filespec":"//depot/main/..."}));
    println!("  {} files tagged", v["data"]["files_tagged"]);

    // ──27. Label revisions ───────────────────────────────────────
    println!("=== 27. Label Revisions ===");
    let v = c.get("/labels/v1.0/revisions");
    assert!(v["data"].as_array().unwrap().len() >= 1);

    // ──28. Stream ────────────────────────────────────────────────
    println!("=== 28. Stream ===");
    c.post_ok("/streams", &json!({"name":"mainline","stream_type":"mainline"}));
    println!("  stream mainline created");

    // ──29. List streams ──────────────────────────────────────────
    println!("=== 29. List Streams ===");
    c.get("/streams");

    // ──30. Protections ───────────────────────────────────────────
    println!("=== 30. Protections ===");
    c.post_ok("/protections", &json!({
        "perm_type":"write","perm_level":"user","entity_type":"user",
        "entity_name":"dev1","depot_path_pattern":"//depot/main/..."
    }));
    println!("  protection added");

    // ──31. List protections ──────────────────────────────────────
    println!("=== 31. List Protections ===");
    let v = c.get("/protections");
    assert!(v["data"].as_array().unwrap().len() >= 1);

    // ──32. Edit + submit second change ───────────────────────────
    println!("=== 32. Edit & Submit Change 2 ===");
    c.post_ok("/clients/smoke-ws/files/edit", &json!({"files":["//depot/main/hello.txt"]}));
    c.post_bytes("/files/content//depot/main/hello.txt", b"Hello Chronoverse v2!");
    let v = c.post_ok("/clients/smoke-ws/changes", &json!({"description":"update hello"}));
    let cid = v["data"]["id"].as_str().unwrap().to_string();
    let v = c.post_ok(&format!("/clients/smoke-ws/changes/{cid}/submit"), &json!({}));
    println!("  change {} submitted", v["data"]["change_number"]);

    // ──33. Verify revision 2 exists ──────────────────────────────
    println!("=== 33. Verify Rev 2 ===");
    let content = c.download("/files/content//depot/main/hello.txt?rev=2");
    assert_eq!(String::from_utf8_lossy(&content), "Hello Chronoverse v2!");
    println!("  rev 2 content verified");

    // ──34. Integration history ───────────────────────────────────
    println!("=== 34. Integration History ===");
    let v = c.get("/integrations?path=//depot/dev/...");
    println!("  {} integration records", v["data"].as_array().unwrap().len());

    // ──35. Info ──────────────────────────────────────────────────
    println!("=== 35. Server Info ===");
    let v = c.get("/info");
    println!("  version: {}", v["data"]["version"]);

    // ──36. Health ────────────────────────────────────────────────
    println!("=== 36. Health ===");
    let v = c.get("/health");
    assert_eq!(v["data"]["status"], "ok");

    println!("\n✅ ALL 36 SMOKE TESTS PASSED");
}
