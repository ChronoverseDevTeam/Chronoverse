#[tokio::test]
async fn test_workspace_crud_and_query() {
	// 在 CI 环境下跳过（避免外部 Mongo 依赖）
	if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
		|| std::env::var("GITHUB_ACTIONS").is_ok()
		|| std::env::var("GITLAB_CI").is_ok() {
		return;
	}

	let _ = crate::config::holder::load_config().await;
	match crate::database::mongo::init_mongo_from_config().await {
		Ok(()) => {}
		Err(crate::database::mongo::MongoError::AlreadyInitialized) => {}
		Err(e) => panic!("init mongo failed: {}", e),
	}

	// unique workspace name
	let mut rnd = [0u8; 8];
	rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
	let wname = format!("test_ws_{:x?}", rnd);

	// create
	let now = chrono::Utc::now();
	let ws = crate::database::workspace::WorkspaceEntity {
		name: wname.clone(),
		created_at: now,
		updated_at: now,
		owner: "tester".to_string(),
		path: "C:/tmp".to_string(),
		device_finger_print: "dev-fp".to_string(),
	};
	crate::database::workspace::create_workspace(ws).await.expect("create_workspace failed");

	// get
	let fetched = crate::database::workspace::get_workspace_by_name(&wname).await.expect("get_workspace_by_name failed");
	assert!(fetched.is_some());

	// list filtered
	let filtered = crate::database::workspace::list_workspaces_filtered(Some(&wname), None, None).await.expect("list_workspaces_filtered failed");
	assert!(filtered.iter().any(|w| w.name == wname));

	// update path
	let ok = crate::database::workspace::update_workspace_path(&wname, "C:/tmp/2").await.expect("update_workspace_path failed");
	assert!(ok);

	// delete
	let ok = crate::database::workspace::delete_workspace(&wname).await.expect("delete_workspace failed");
	assert!(ok);

	// ensure gone
	let fetched = crate::database::workspace::get_workspace_by_name(&wname).await.expect("get_workspace_by_name failed");
	assert!(fetched.is_none());
}

