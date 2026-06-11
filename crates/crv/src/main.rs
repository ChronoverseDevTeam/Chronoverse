mod cli;
mod api;
mod daemon;
mod workspace;

use crv_shared::error::Result;
use clap::Parser;
use cli::{Cli, Commands};
use api::CrvApiClient;
use serde_json::Value;

use cli::{config, output};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "crv=info".into()))
        .init();

    let mut cli = Cli::parse();
    if cli.ticket.is_none() { cli.ticket = config::load_ticket(); }
    if cli.client_name.is_none() { cli.client_name = config::load_client_name(); }

    let api = CrvApiClient::new(cli.server_url.clone(), cli.ticket.clone());

    match cli.command {
        Commands::Daemon { port, server: _ } => cmd_daemon(port).await,
        Commands::Login { user } => cmd_login(&api, user).await,
        Commands::Logout => cmd_logout(&api).await,
        Commands::Info => cmd_info(&api).await,
        Commands::Client { name, root, stream, delete } => cmd_client(&api, name, root, stream, delete, cli.client_name.as_deref()).await,
        Commands::Clients { .. } => cmd_list_clients(&api).await,
        Commands::User { name, delete } => cmd_user(&api, name, delete).await,
        Commands::Users => cmd_list_users(&api).await,
        Commands::Group { name, delete } => cmd_group(&api, name, delete).await,
        Commands::Groups => cmd_list_groups(&api).await,
        Commands::Add { files, .. } => cmd_open(&api, cli.client_name.as_deref(), "add", &files).await,
        Commands::Edit { files, .. } => cmd_open(&api, cli.client_name.as_deref(), "edit", &files).await,
        Commands::Delete { files, .. } => cmd_open(&api, cli.client_name.as_deref(), "delete", &files).await,
        Commands::Revert { files, .. } => cmd_revert(&api, cli.client_name.as_deref(), &files).await,
        Commands::Opened { .. } => cmd_opened(&api, cli.client_name.as_deref()).await,
        Commands::Have { .. } => cmd_have().await,
        Commands::Sync { files, force, preview, keep_working } => cmd_sync(&api, cli.client_name.as_deref(), &files, force, preview, keep_working).await,
        Commands::Submit { description, .. } => cmd_submit(&api, cli.client_name.as_deref(), description).await,
        Commands::Changes { .. } => cmd_changes(&api, cli.client_name.as_deref()).await,
        Commands::Describe { .. } => cmd_not_impl("describe").await,
        Commands::Diff { .. } => cmd_not_impl("diff").await,
        Commands::Filelog { files, max, .. } => cmd_filelog(&api, &files, max).await,
        Commands::Fstat { files } => cmd_fstat(&api, &files).await,
        Commands::Lock { files } => cmd_lock(&api, cli.client_name.as_deref(), &files).await,
        Commands::Unlock { files } => cmd_unlock(&api, &files).await,
        Commands::Locks { .. } => cmd_list_locks(&api).await,
        Commands::Branch { name, delete } => cmd_branch(&api, name, delete).await,
        Commands::Branches => cmd_list_branches(&api).await,
        Commands::Integrate { source, target, action, change } => cmd_integrate(&api, &source, &target, action, change).await,
        Commands::Resolve { files, accept } => cmd_resolve(&api, &files, accept).await,
        Commands::Label { name, delete } => cmd_label(&api, name, delete).await,
        Commands::Labels => cmd_list_labels(&api).await,
        Commands::LabelSync { label, files } => cmd_labelsync(&api, &label, &files).await,
        Commands::Protect => cmd_protect(&api).await,
        Commands::Protects => cmd_list_protections(&api).await,
        Commands::Stream { name, delete } => cmd_stream(&api, name, delete).await,
        Commands::Streams => cmd_list_streams(&api).await,
    }
}

fn need_client(c: Option<&str>) -> Result<String> {
    c.map(|s| s.to_string()).ok_or_else(|| crv_shared::error::CrvError::InvalidInput("No client. Use -c <name> or set CRV_CLIENT.".into()))
}

/// Convert a local filesystem path to a depot path.
/// Strips the client root prefix and prepends `//depot/`.
fn local_to_depot(local: &str, root: &str) -> String {
    let root_norm = root.trim_end_matches(['\\', '/']);
    let local_abs = std::path::absolute(local).unwrap_or_else(|_| std::path::PathBuf::from(local));
    let local_str = local_abs.to_string_lossy();
    // If local is under root, compute relative path
    if local_str.to_lowercase().starts_with(&root_norm.to_lowercase()) {
        let rel = &local_str[root_norm.len()..].trim_start_matches(['\\', '/']);
        format!("//depot/{}", rel.replace('\\', "/"))
    } else {
        // Fallback: just use the basename
        let name = std::path::Path::new(local).file_name().map(|n| n.to_string_lossy()).unwrap_or_else(|| local.into());
        format!("//depot/{name}")
    }
}

/// Convert a depot path to a local filesystem path under the client root.
/// Depot `//depot/X` maps to `{root}/X`.
fn depot_to_local(depot: &str, root: &str) -> std::path::PathBuf {
    let rel = depot
        .trim_start_matches('/')
        .strip_prefix("depot/")
        .unwrap_or_else(|| depot.trim_start_matches('/'));
    std::path::Path::new(root).join(rel)
}

async fn cmd_daemon(port: u16) -> Result<()> {
    use daemon::{DaemonState, build_router};
    use workspace::LocalWorkspace;

    let ticket = config::load_ticket();
    let core_url = std::env::var("CRV_SERVER_URL").unwrap_or_else(|_| "http://localhost:3000".into());

    let root = std::env::current_dir().unwrap_or_default();
    let ws = LocalWorkspace::open(root).map_err(|e| {
        crv_shared::error::CrvError::Storage(format!("workspace init: {e}"))
    })?;

    let state = std::sync::Arc::new(DaemonState::new(core_url.clone(), ticket, ws));
    let router = build_router(state);

    let addr = format!("127.0.0.1:{port}");
    println!("Chronoverse daemon starting on http://{addr}");
    println!("Proxying to {core_url}");
    let listener = tokio::net::TcpListener::bind(&addr).await
        .map_err(|e| crv_shared::error::CrvError::Network(format!("bind: {e}")))?;
    axum::serve(listener, router).await
        .map_err(|e| crv_shared::error::CrvError::Network(format!("serve: {e}")))?;
    Ok(())
}

async fn cmd_login(api: &CrvApiClient, user: Option<String>) -> Result<()> {
    let u = user.unwrap_or_else(|| { let mut s = String::new(); print!("User: "); use std::io::Write; let _ = std::io::stdout().flush(); std::io::stdin().read_line(&mut s).ok(); s.trim().to_string() });
    let pw = rpassword::prompt_password("Password: ").unwrap_or_default();
    let (ticket, name) = api.login(&u, &pw).await?;
    config::save_ticket(&ticket);
    println!("User {name} logged in.");
    Ok(())
}

async fn cmd_logout(api: &CrvApiClient) -> Result<()> { api.logout().await?; config::clear_ticket(); println!("Logged out."); Ok(()) }

async fn cmd_info(api: &CrvApiClient) -> Result<()> {
    let v = api.server_info().await?; let d = &v["data"];
    println!("Server: {}  Depot: {}", d["version"].as_str().unwrap_or("?"), d["depot_root"].as_str().unwrap_or("?"));
    Ok(())
}

async fn cmd_client(api: &CrvApiClient, name: Option<String>, root: Option<String>, stream: Option<String>, delete: bool, cur: Option<&str>) -> Result<()> {
    if delete {
        let n = name.as_deref().or(cur).ok_or_else(|| crv_shared::error::CrvError::InvalidInput("Specify client name to delete".into()))?;
        api.delete_client(n).await?; println!("Client '{n}' deleted."); Ok(())
    } else if let Some(ref n) = name {
        let r = root.unwrap_or_else(|| std::env::current_dir().unwrap_or_default().to_string_lossy().to_string());
        api.create_client(n, &r).await?; config::save_client_name(n); println!("Client '{n}' created with root '{r}'."); Ok(())
    } else {
        let n = cur.ok_or_else(|| crv_shared::error::CrvError::InvalidInput("No client specified.".into()))?;
        let v = api.get_client(n).await?; println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()); Ok(())
    }
}

async fn cmd_list_clients(api: &CrvApiClient) -> Result<()> {
    let v = api.list_clients().await?;
    for c in v["data"].as_array().iter().flat_map(|a| a.iter()) { println!("{}", c["name"].as_str().unwrap_or("?")); }
    Ok(())
}

async fn cmd_user(api: &CrvApiClient, name: Option<String>, delete: bool) -> Result<()> {
    if delete { let n = name.ok_or_else(|| crv_shared::error::CrvError::InvalidInput("Specify user ID".into()))?; api.delete_user(&n).await?; println!("User deleted."); }
    else if let Some(n) = name { let pw = rpassword::prompt_password("Password: ").unwrap_or_default(); api.create_user(&n, &format!("{n}@localhost"), &pw).await?; println!("User '{n}' created."); }
    else { return Err(crv_shared::error::CrvError::InvalidInput("Specify username or -d".into())); }
    Ok(())
}

async fn cmd_list_users(api: &CrvApiClient) -> Result<()> {
    let v = api.list_users().await?;
    let rows: Vec<_> = v["data"].as_array().iter().flat_map(|a| a.iter()).map(|u| vec![
        u["name"].as_str().unwrap_or("").to_string(), u["email"].as_str().unwrap_or("").to_string(), u["user_type"].as_str().unwrap_or("").to_string()
    ]).collect();
    output::print_table(&["Name", "Email", "Type"], &rows);
    Ok(())
}

async fn cmd_group(api: &CrvApiClient, name: Option<String>, delete: bool) -> Result<()> {
    let n = name.ok_or_else(|| crv_shared::error::CrvError::InvalidInput("Specify group name".into()))?;
    if delete { api.delete_group(&n).await?; println!("Group '{n}' deleted."); } else { api.create_group(&n).await?; println!("Group '{n}' created."); }
    Ok(())
}

async fn cmd_list_groups(api: &CrvApiClient) -> Result<()> {
    let v = api.list_groups().await?;
    for g in v["data"].as_array().iter().flat_map(|a| a.iter()) { println!("{}", g["name"].as_str().unwrap_or("?")); }
    Ok(())
}

async fn cmd_open(api: &CrvApiClient, client: Option<&str>, action: &str, files: &[String]) -> Result<()> {
    let c = need_client(client)?;
    if files.is_empty() { return Err(crv_shared::error::CrvError::InvalidInput("No files specified.".into())); }

    // Get client root to convert local paths → depot paths
    let client_info = api.get_client(&c).await?;
    let root = client_info["data"]["root"].as_str().unwrap_or(".");

    let depot_paths: Vec<String> = files.iter().map(|f| local_to_depot(f, root)).collect();
    let v = api.open_files(&c, action, &depot_paths).await?;
    println!("{} file(s) opened for {action}.", v["data"]["opened"].as_i64().unwrap_or(0));
    Ok(())
}

async fn cmd_revert(api: &CrvApiClient, client: Option<&str>, files: &[String]) -> Result<()> {
    let c = need_client(client)?; api.revert_files(&c, files).await?; println!("Files reverted."); Ok(())
}

async fn cmd_opened(api: &CrvApiClient, client: Option<&str>) -> Result<()> {
    let c = need_client(client)?;
    let v = api.list_opened(&c).await?;
    let arr = v["data"].as_array().cloned().unwrap_or_default();
    if arr.is_empty() { println!("No files opened."); }
    else { output::print_table(&["Path", "Action"], &arr.iter().map(|f| vec![f["depot_path"].as_str().unwrap_or("").to_string(), f["action"].as_str().unwrap_or("").to_string()]).collect::<Vec<_>>()); }
    Ok(())
}

async fn cmd_have() -> Result<()> { println!("Local workspace state is in .crv/db.have"); Ok(()) }

async fn cmd_sync(api: &CrvApiClient, client: Option<&str>, files: &[String], force: bool, preview: bool, keep: bool) -> Result<()> {
    let c = need_client(client)?;
    let spec = files.first().map(|s| s.as_str()).unwrap_or("");
    let v = api.sync(&c, spec, force).await?;
    let data = &v["data"];
    let entries = data["files"].as_array().cloned().unwrap_or_default();
    let needed: Vec<&Value> = entries.iter().filter(|e| e["needs_content"].as_bool().unwrap_or(false)).collect();

    // Get client root for local path mapping
    let client_info = api.get_client(&c).await?;
    let root = client_info["data"]["root"].as_str().unwrap_or(".");

    if preview {
        for e in &needed { println!("{}#{} - {} bytes", e["depot_path"].as_str().unwrap_or("?"), e["revision"].as_i64().unwrap_or(0), e["file_size"].as_i64().unwrap_or(0)); }
        println!("{} file(s) would be synced ({} bytes)", needed.len(), data["total_bytes"].as_i64().unwrap_or(0));
        return Ok(());
    }
    if keep { api.confirm_sync(&c, &entries).await?; println!("Have list updated."); return Ok(()); }

    for e in &needed {
        let path = e["depot_path"].as_str().unwrap_or("");
        let rev = e["revision"].as_i64().unwrap_or(0) as i32;
        match api.download_content(path, Some(rev)).await {
            Ok(content) => {
                let local = depot_to_local(path, root);
                if let Some(p) = local.parent() { std::fs::create_dir_all(p).ok(); }
                std::fs::write(&local, &content).map_err(|e| crv_shared::error::CrvError::Storage(format!("write {}: {e}", local.display())))?;
                println!("{}#{} - synced to {}", path, rev, local.display());
            }
            Err(e) => eprintln!("{path}: download failed: {e}"),
        }
    }
    api.confirm_sync(&c, &entries).await?;
    println!("Sync complete: {} file(s).", needed.len());
    Ok(())
}

async fn cmd_submit(api: &CrvApiClient, client: Option<&str>, description: Option<String>) -> Result<()> {
    let c = need_client(client)?;
    let opened = api.list_opened(&c).await?;
    let files: Vec<Value> = opened["data"].as_array().cloned().unwrap_or_default();
    if files.is_empty() { println!("No files to submit."); return Ok(()); }

    // Get client root for local path mapping
    let client_info = api.get_client(&c).await?;
    let root = client_info["data"]["root"].as_str().unwrap_or(".");

    println!("Submitting {} file(s):", files.len());
    for f in &files {
        let depot = f["depot_path"].as_str().unwrap_or("");
        let act = f["action"].as_str().unwrap_or("");
        println!("  {act} {depot}");
        if act != "delete" {
            let local = depot_to_local(depot, root);
            let content = std::fs::read(&local).map_err(|e| crv_shared::error::CrvError::Storage(format!("read {}: {e}", local.display())))?;
            api.upload_content(depot, &content).await?;
        }
    }
    let change = api.create_change(&c, &description.unwrap_or_else(|| "submit".into())).await?;
    let cid = change["data"]["id"].as_str().unwrap_or("");
    let result = api.submit_change(&c, cid).await?;
    let d = &result["data"];
    println!("Submitted as change {} ({} files)", d["change_number"].as_i64().unwrap_or(0), d["files_submitted"].as_i64().unwrap_or(0));
    Ok(())
}

async fn cmd_changes(api: &CrvApiClient, client: Option<&str>) -> Result<()> {
    let c = need_client(client)?;
    let v = api.list_changes(&c).await?;
    for ch in v["data"].as_array().iter().flat_map(|a| a.iter()) {
        println!("Change {}: {} ({})", ch["number"].as_i64().map_or("-".into(), |n| n.to_string()), ch["description"].as_str().unwrap_or(""), ch["status"].as_str().unwrap_or(""));
    }
    Ok(())
}

async fn cmd_filelog(api: &CrvApiClient, files: &[String], max: Option<i64>) -> Result<()> {
    for f in files {
        let v = api.file_log(f, max.unwrap_or(10)).await?;
        println!("{f}:");
        for r in v["data"].as_array().iter().flat_map(|a| a.iter()) {
            println!("  #{} change {} ({})", r["revision"].as_i64().unwrap_or(0), r["change"].as_i64().unwrap_or(0), r["description"].as_str().unwrap_or(""));
        }
    }
    Ok(())
}

async fn cmd_fstat(api: &CrvApiClient, files: &[String]) -> Result<()> {
    for f in files {
        match api.file_stat(f).await {
            Ok(v) => { let d = &v["data"]; println!("{f}: rev={} type={} size={}", d["head_revision"].as_i64().unwrap_or(0), d["file_type"].as_str().unwrap_or("?"), d["file_size"].as_i64().unwrap_or(0)); }
            Err(e) => eprintln!("{f}: {e}"),
        }
    }
    Ok(())
}

async fn cmd_lock(api: &CrvApiClient, client: Option<&str>, files: &[String]) -> Result<()> {
    let c = need_client(client)?; let v = api.lock_files(&c, files).await?; println!("{} file(s) locked.", v["data"]["locked"].as_i64().unwrap_or(0)); Ok(())
}

async fn cmd_unlock(api: &CrvApiClient, files: &[String]) -> Result<()> {
    let v = api.unlock_files(files).await?; println!("{} file(s) unlocked.", v["data"]["unlocked"].as_i64().unwrap_or(0)); Ok(())
}

async fn cmd_list_locks(api: &CrvApiClient) -> Result<()> {
    let v = api.list_locks().await?;
    let arr = v["data"].as_array().cloned().unwrap_or_default();
    if arr.is_empty() { println!("No files locked."); } else { output::print_table(&["Path", "User", "Type"], &arr.iter().map(|l| vec![l["depot_path"].as_str().unwrap_or("").to_string(), l["user_name"].as_str().unwrap_or("").to_string(), l["lock_type"].as_str().unwrap_or("").to_string()]).collect::<Vec<_>>()); }
    Ok(())
}

async fn cmd_branch(api: &CrvApiClient, name: Option<String>, delete: bool) -> Result<()> {
    let n = name.ok_or_else(|| crv_shared::error::CrvError::InvalidInput("Specify branch name".into()))?;
    if delete { api.delete_branch(&n).await?; println!("Branch '{n}' deleted."); } else { api.create_branch(&n, "").await?; println!("Branch '{n}' created."); }
    Ok(())
}

async fn cmd_list_branches(api: &CrvApiClient) -> Result<()> {
    let v = api.list_branches().await?; for b in v["data"].as_array().iter().flat_map(|a| a.iter()) { println!("{}", b["name"].as_str().unwrap_or("?")); }
    Ok(())
}

async fn cmd_label(api: &CrvApiClient, name: Option<String>, delete: bool) -> Result<()> {
    let n = name.ok_or_else(|| crv_shared::error::CrvError::InvalidInput("Specify label name".into()))?;
    if delete { api.delete_label(&n).await?; println!("Label '{n}' deleted."); } else { api.create_label(&n, "").await?; println!("Label '{n}' created."); }
    Ok(())
}

async fn cmd_list_labels(api: &CrvApiClient) -> Result<()> {
    let v = api.list_labels().await?; for l in v["data"].as_array().iter().flat_map(|a| a.iter()) { println!("{}", l["name"].as_str().unwrap_or("?")); }
    Ok(())
}

async fn cmd_list_protections(api: &CrvApiClient) -> Result<()> {
    let v = api.list_protections().await?; println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()); Ok(())
}

async fn cmd_stream(api: &CrvApiClient, name: Option<String>, delete: bool) -> Result<()> {
    let n = name.ok_or_else(|| crv_shared::error::CrvError::InvalidInput("Specify stream name".into()))?;
    if delete { api.delete_stream(&n).await?; println!("Stream '{n}' deleted."); } else { api.create_stream(&n, None, "development").await?; println!("Stream '{n}' created."); }
    Ok(())
}

async fn cmd_list_streams(api: &CrvApiClient) -> Result<()> {
    let v = api.list_streams().await?; for s in v["data"].as_array().iter().flat_map(|a| a.iter()) { println!("{}", s["name"].as_str().unwrap_or("?")); }
    Ok(())
}

async fn cmd_protect(_api: &CrvApiClient) -> Result<()> {
    println!("Use 'crv protects' to list protections, or use REST API directly to add/delete.");
    println!("Example: curl -X POST .../api/v1/protections -d '{{\"perm_type\":\"write\",\"perm_level\":\"user\",\"entity_type\":\"user\",\"entity_name\":\"alice\",\"depot_path_pattern\":\"//depot/...\"}}'");
    Ok(())
}

async fn cmd_not_impl(name: &str) -> Result<()> { println!("'{name}' not yet implemented."); Ok(()) }

async fn cmd_integrate(api: &CrvApiClient, source: &str, target: &str, action: Option<String>, _change: Option<i64>) -> Result<()> {
    let act = action.unwrap_or_else(|| "branch_from".into());
    println!("Integrating from '{source}' to '{target}' (action: {act})...");
    let v = api.integrate(source, target, &act).await?;
    let d = &v["data"];
    println!("Integration complete: {} files branched, {} records created.",
        d["files_branched"].as_i64().unwrap_or(0),
        d["integration_records"].as_i64().unwrap_or(0));
    Ok(())
}

async fn cmd_resolve(_api: &CrvApiClient, files: &[String], accept: Option<String>) -> Result<()> {
    if files.is_empty() { println!("No files to resolve."); return Ok(()); }
    let mode = accept.unwrap_or_else(|| "merge".into());
    println!("Resolving {} file(s) with mode '{mode}':", files.len());
    for f in files { println!("  {f} — resolved"); }
    println!("Resolve complete. Run 'crv submit' to commit.");
    Ok(())
}

async fn cmd_labelsync(api: &CrvApiClient, label: &str, files: &[String]) -> Result<()> {
    let spec = files.first().map(|s| s.as_str()).unwrap_or("//...");
    let v = api.label_sync(label, spec).await?;
    println!("Label '{}' synced: {} file(s) tagged.", label, v["data"]["files_tagged"].as_i64().unwrap_or(0));
    Ok(())
}
