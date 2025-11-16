// gRPC 相关的集成测试
// 注意：这些测试直接调用 logic 层和 hive_server 的处理函数，而不启动真实的 gRPC 服务器
// 如需完整的端到端测试，请运行 crv-hive 服务器后使用 crv-cli 进行测试

use tonic::Request;

#[tokio::test]
async fn test_auth_logic_flow() {
    // 在 CI 环境下跳过
    if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
        return;
    }

    // 初始化配置和数据库
    let _ = crate::config::holder::load_config().await;
    match crate::database::mongo::init_mongo_from_config().await {
        Ok(()) => {}
        Err(crate::database::mongo::MongoError::AlreadyInitialized) => {}
        Err(e) => panic!("init mongo failed: {}", e),
    }

    // 生成唯一用户名（使用时间戳和随机数确保唯一性）
    let mut rnd = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
    let timestamp = chrono::Utc::now().timestamp_millis();
    let username = format!("test_auth_{}_{:x?}", timestamp, rnd);

    // 预清理：确保用户不存在
    let _ = crate::database::user::delete_user(&username).await;

    // 1. 测试注册
    let register_req = Request::new(crate::pb::RegisterReq {
        username: username.clone(),
        password: "test_password".to_string(),
        email: "test@example.com".to_string(),
    });
    let register_resp = crate::logic::auth::register(register_req).await;
    if let Err(e) = &register_resp {
        eprintln!("注册失败: {:?}", e);
    }
    assert!(register_resp.is_ok(), "注册应该成功");

    // 2. 测试登录
    let login_req = Request::new(crate::pb::LoginReq {
        username: username.clone(),
        password: "test_password".to_string(),
    });
    let login_resp = crate::logic::auth::login(login_req).await;
    assert!(login_resp.is_ok(), "登录应该成功");

    let login_data = login_resp.unwrap().into_inner();
    assert!(!login_data.access_token.is_empty(), "应该返回 access token");
    assert!(login_data.expires_at > 0, "应该有过期时间");

    // 3. 测试错误密码登录
    let wrong_login_req = Request::new(crate::pb::LoginReq {
        username: username.clone(),
        password: "wrong_password".to_string(),
    });
    let wrong_login_resp = crate::logic::auth::login(wrong_login_req).await;
    assert!(wrong_login_resp.is_err(), "错误密码登录应该失败");
    assert_eq!(
        wrong_login_resp.unwrap_err().code(),
        tonic::Code::Unauthenticated
    );

    // 4. 测试重复注册
    let duplicate_register_req = Request::new(crate::pb::RegisterReq {
        username: username.clone(),
        password: "test_password".to_string(),
        email: "test2@example.com".to_string(),
    });
    let duplicate_resp = crate::logic::auth::register(duplicate_register_req).await;
    assert!(duplicate_resp.is_err(), "重复注册应该失败");
    assert_eq!(
        duplicate_resp.unwrap_err().code(),
        tonic::Code::AlreadyExists
    );

    // 清理：删除测试用户
    let _ = crate::database::user::delete_user(&username).await;
}

#[tokio::test]
async fn test_workspace_logic_operations() {
    // 在 CI 环境下跳过
    if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
        return;
    }

    // 初始化配置和数据库
    let _ = crate::config::holder::load_config().await;
    match crate::database::mongo::init_mongo_from_config().await {
        Ok(()) => {}
        Err(crate::database::mongo::MongoError::AlreadyInitialized) => {}
        Err(e) => panic!("init mongo failed: {}", e),
    }

    // 生成唯一用户名和工作区名
    let mut rnd = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
    let timestamp = chrono::Utc::now().timestamp_millis();
    let username = format!("test_ws_user_{}_{:x?}", timestamp, rnd);
    let ws_name = format!("test_ws_{}_{:x?}", timestamp, rnd);

    // 预清理
    let _ = crate::database::user::delete_user(&username).await;
    let _ = crate::database::workspace::delete_workspace(&ws_name).await;

    // 1. 创建测试用户
    let now = chrono::Utc::now();
    let user = crate::database::user::UserEntity {
        name: username.clone(),
        email: "test@example.com".to_string(),
        password: "hashed_password".to_string(),
        created_at: now,
        updated_at: now,
    };
    crate::database::user::create_user(user)
        .await
        .expect("创建用户失败");

    // 2. 使用 middleware 创建带用户上下文的请求
    let mut upsert_ws_req = Request::new(crate::pb::UpsertWorkspaceReq {
        name: ws_name.clone(),
        path: "C:/test/path".to_string(),
        device_finger_print: "test-device".to_string(),
    });

    // 手动设置用户上下文（模拟 middleware 的行为）
    upsert_ws_req
        .extensions_mut()
        .insert(crate::middleware::UserContext {
            username: username.clone(),
            scopes: vec![],
            source: "test",
        });

    // 3. 测试 Upsert 工作区（创建）
    let create_resp = crate::logic::create_workspace::upsert_workspace(upsert_ws_req).await;
    assert!(create_resp.is_ok(), "Upsert 工作区应该成功");

    // 4. 测试列出工作区（按名称过滤）
    let list_req = Request::new(crate::pb::ListWorkspaceReq {
        name: Some(ws_name.clone()),
        owner: None,
        device_finger_print: None,
    });
    let list_resp = crate::logic::list_workspaces::list_workspaces(list_req).await;
    assert!(list_resp.is_ok(), "列出工作区应该成功");

    let workspaces = list_resp.unwrap().into_inner().workspaces;
    assert!(
        workspaces.iter().any(|w| w.name == ws_name),
        "应该找到创建的工作区"
    );

    // 5. 测试列出工作区（按 owner 过滤）
    let list_by_owner_req = Request::new(crate::pb::ListWorkspaceReq {
        name: None,
        owner: Some(username.clone()),
        device_finger_print: None,
    });
    let list_by_owner_resp =
        crate::logic::list_workspaces::list_workspaces(list_by_owner_req).await;
    assert!(list_by_owner_resp.is_ok(), "按 owner 列出工作区应该成功");

    let workspaces_by_owner = list_by_owner_resp.unwrap().into_inner().workspaces;
    assert!(
        workspaces_by_owner.iter().any(|w| w.name == ws_name),
        "应该通过 owner 找到工作区"
    );

    // 6. 测试 Upsert：重复创建工作区（会更新现有工作区）
    let mut update_ws_req = Request::new(crate::pb::UpsertWorkspaceReq {
        name: ws_name.clone(),
        path: "C:/test/path2".to_string(), // 更新路径
        device_finger_print: "test-device-updated".to_string(), // 更新设备指纹
    });
    update_ws_req
        .extensions_mut()
        .insert(crate::middleware::UserContext {
            username: username.clone(),
            scopes: vec![],
            source: "test",
        });

    let upsert_resp = crate::logic::create_workspace::upsert_workspace(update_ws_req).await;
    assert!(upsert_resp.is_ok(), "Upsert 应该成功");

    // 验证工作区被更新了
    let updated_workspace = crate::database::workspace::get_workspace_by_name(&ws_name)
        .await
        .expect("获取工作区失败")
        .expect("工作区应该存在");
    assert_eq!(updated_workspace.path, "C:/test/path2", "路径应该被更新");
    assert_eq!(
        updated_workspace.device_finger_print, "test-device-updated",
        "设备指纹应该被更新"
    );

    // 清理：删除测试数据
    let _ = crate::database::workspace::delete_workspace(&ws_name).await;
    let _ = crate::database::user::delete_user(&username).await;
}

#[tokio::test]
async fn test_workspace_validation() {
    // 在 CI 环境下跳过
    if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
        return;
    }

    // 初始化配置和数据库
    let _ = crate::config::holder::load_config().await;
    match crate::database::mongo::init_mongo_from_config().await {
        Ok(()) => {}
        Err(crate::database::mongo::MongoError::AlreadyInitialized) => {}
        Err(e) => panic!("init mongo failed: {}", e),
    }

    // 生成唯一用户名
    let mut rnd = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
    let timestamp = chrono::Utc::now().timestamp_millis();
    let username = format!("test_val_{}_{:x?}", timestamp, rnd);

    // 1. 测试空名称
    let mut req = Request::new(crate::pb::UpsertWorkspaceReq {
        name: "".to_string(),
        path: "C:/test/path".to_string(),
        device_finger_print: "test-device".to_string(),
    });
    req.extensions_mut().insert(crate::middleware::UserContext {
        username: username.clone(),
        scopes: vec![],
        source: "test",
    });
    let resp = crate::logic::create_workspace::upsert_workspace(req).await;
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);

    // 2. 测试空路径
    let mut req = Request::new(crate::pb::UpsertWorkspaceReq {
        name: "valid_name".to_string(),
        path: "".to_string(),
        device_finger_print: "test-device".to_string(),
    });
    req.extensions_mut().insert(crate::middleware::UserContext {
        username: username.clone(),
        scopes: vec![],
        source: "test",
    });
    let resp = crate::logic::create_workspace::upsert_workspace(req).await;
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);

    // 3. 测试空设备指纹
    let mut req = Request::new(crate::pb::UpsertWorkspaceReq {
        name: "valid_name".to_string(),
        path: "C:/test/path".to_string(),
        device_finger_print: "".to_string(),
    });
    req.extensions_mut().insert(crate::middleware::UserContext {
        username: username.clone(),
        scopes: vec![],
        source: "test",
    });
    let resp = crate::logic::create_workspace::upsert_workspace(req).await;
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);

    // 4. 测试 ListWorkspaces 无过滤条件
    let req = Request::new(crate::pb::ListWorkspaceReq {
        name: None,
        owner: None,
        device_finger_print: None,
    });
    let resp = crate::logic::list_workspaces::list_workspaces(req).await;
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn test_auth_validation() {
    // 在 CI 环境下跳过
    if std::env::var("CI").map(|v| v == "true").unwrap_or(false)
        || std::env::var("GITHUB_ACTIONS").is_ok()
        || std::env::var("GITLAB_CI").is_ok()
    {
        return;
    }

    // 初始化配置和数据库
    let _ = crate::config::holder::load_config().await;
    match crate::database::mongo::init_mongo_from_config().await {
        Ok(()) => {}
        Err(crate::database::mongo::MongoError::AlreadyInitialized) => {}
        Err(e) => panic!("init mongo failed: {}", e),
    }

    // 1. 测试空用户名注册
    let req = Request::new(crate::pb::RegisterReq {
        username: "".to_string(),
        password: "password".to_string(),
        email: "test@example.com".to_string(),
    });
    let resp = crate::logic::auth::register(req).await;
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);

    // 2. 测试空密码注册
    let req = Request::new(crate::pb::RegisterReq {
        username: "testuser".to_string(),
        password: "".to_string(),
        email: "test@example.com".to_string(),
    });
    let resp = crate::logic::auth::register(req).await;
    assert!(resp.is_err());
    assert_eq!(resp.unwrap_err().code(), tonic::Code::InvalidArgument);

    // 3. 测试不存在的用户登录
    let req = Request::new(crate::pb::LoginReq {
        username: "nonexistent_user_12345".to_string(),
        password: "password".to_string(),
    });
    let resp = crate::logic::auth::login(req).await;
    assert!(resp.is_err());
    // 不存在的用户可能返回 Internal（数据库查询）或 Unauthenticated
    let code = resp.unwrap_err().code();
    assert!(
        code == tonic::Code::Unauthenticated || code == tonic::Code::Internal,
        "应该返回 Unauthenticated 或 Internal 错误"
    );
}
