#[tokio::test]
async fn test_workspace_crud_and_query() {
    // 在 CI 环境下跳过（避免外部 Mongo 依赖）
    if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
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
    crate::database::workspace::create_workspace(ws)
        .await
        .expect("create_workspace failed");

    // get
    let fetched = crate::database::workspace::get_workspace_by_name(&wname)
        .await
        .expect("get_workspace_by_name failed");
    assert!(fetched.is_some());

    // list filtered
    let filtered = crate::database::workspace::list_workspaces_filtered(Some(&wname), None, None)
        .await
        .expect("list_workspaces_filtered failed");
    assert!(filtered.iter().any(|w| w.name == wname));

    // update path
    let ok = crate::database::workspace::update_workspace_path(&wname, "C:/tmp/2")
        .await
        .expect("update_workspace_path failed");
    assert!(ok);

    // delete
    let ok = crate::database::workspace::delete_workspace(&wname)
        .await
        .expect("delete_workspace failed");
    assert!(ok);

    // ensure gone
    let fetched = crate::database::workspace::get_workspace_by_name(&wname)
        .await
        .expect("get_workspace_by_name failed");
    assert!(fetched.is_none());
}

#[tokio::test]
async fn test_list_workspaces_logic() {
    // 在 CI 环境下跳过（避免外部 Mongo 依赖）
    if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
        return;
    }

    let _ = crate::config::holder::load_config().await;
    match crate::database::mongo::init_mongo_from_config().await {
        Ok(()) => {}
        Err(crate::database::mongo::MongoError::AlreadyInitialized) => {}
        Err(e) => panic!("init mongo failed: {}", e),
    }

    // 创建测试用的工作区
    let mut rnd = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
    let wname1 = format!("test_list_ws1_{:x?}", rnd);
    let wname2 = format!("test_list_ws2_{:x?}", rnd);
    let owner = "test_list_owner";
    let device_fp = "test-device-fp";

    let now = chrono::Utc::now();

    // 创建第一个工作区
    let ws1 = crate::database::workspace::WorkspaceEntity {
        name: wname1.clone(),
        created_at: now,
        updated_at: now,
        owner: owner.to_string(),
        path: "C:/tmp/ws1".to_string(),
        device_finger_print: device_fp.to_string(),
    };
    crate::database::workspace::create_workspace(ws1)
        .await
        .expect("create_workspace failed");

    // 创建第二个工作区
    let ws2 = crate::database::workspace::WorkspaceEntity {
        name: wname2.clone(),
        created_at: now,
        updated_at: now,
        owner: owner.to_string(),
        path: "C:/tmp/ws2".to_string(),
        device_finger_print: device_fp.to_string(),
    };
    crate::database::workspace::create_workspace(ws2)
        .await
        .expect("create_workspace failed");

    // 测试 list_workspaces logic - 按名称过滤
    {
        use crate::pb::ListWorkspaceReq;
        use tonic::Request;

        let req = ListWorkspaceReq {
            name: Some(wname1.clone()),
            owner: None,
            device_finger_print: None,
        };
        let request = Request::new(req);
        let response = crate::logic::list_workspaces::list_workspaces(request).await;
        assert!(response.is_ok());
        let resp = response.unwrap().into_inner();
        assert!(resp.workspaces.iter().any(|w| w.name == wname1));
    }

    // 测试 list_workspaces logic - 按 owner 过滤
    {
        use crate::pb::ListWorkspaceReq;
        use tonic::Request;

        let req = ListWorkspaceReq {
            name: None,
            owner: Some(owner.to_string()),
            device_finger_print: None,
        };
        let request = Request::new(req);
        let response = crate::logic::list_workspaces::list_workspaces(request).await;
        assert!(response.is_ok());
        let resp = response.unwrap().into_inner();
        let found_workspaces: Vec<_> = resp
            .workspaces
            .iter()
            .filter(|w| w.name == wname1 || w.name == wname2)
            .collect();
        assert!(
            found_workspaces.len() >= 2,
            "should find at least both test workspaces"
        );
    }

    // 测试 list_workspaces logic - 按 device_finger_print 过滤
    {
        use crate::pb::ListWorkspaceReq;
        use tonic::Request;

        let req = ListWorkspaceReq {
            name: None,
            owner: None,
            device_finger_print: Some(device_fp.to_string()),
        };
        let request = Request::new(req);
        let response = crate::logic::list_workspaces::list_workspaces(request).await;
        assert!(response.is_ok());
        let resp = response.unwrap().into_inner();
        let found_workspaces: Vec<_> = resp
            .workspaces
            .iter()
            .filter(|w| w.name == wname1 || w.name == wname2)
            .collect();
        assert!(
            found_workspaces.len() >= 2,
            "should find at least both test workspaces"
        );
    }

    // 测试无过滤条件应该失败
    {
        use crate::pb::ListWorkspaceReq;
        use tonic::Request;

        let req = ListWorkspaceReq {
            name: None,
            owner: None,
            device_finger_print: None,
        };
        let request = Request::new(req);
        let response = crate::logic::list_workspaces::list_workspaces(request).await;
        assert!(response.is_err(), "should fail without any filter");
        let err = response.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    // 清理测试数据
    let _ = crate::database::workspace::delete_workspace(&wname1).await;
    let _ = crate::database::workspace::delete_workspace(&wname2).await;
}
