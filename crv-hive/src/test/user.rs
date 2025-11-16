#[tokio::test]
async fn test_user_crud() {
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

    // unique user name
    let mut rnd = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut rnd);
    let uname = format!("test_user_{:x?}", rnd);

    // create
    let now = chrono::Utc::now();
    let user = crate::database::user::UserEntity {
        name: uname.clone(),
        email: "user@test.local".to_string(),
        password: "pass".to_string(),
        created_at: now,
        updated_at: now,
    };
    crate::database::user::create_user(user)
        .await
        .expect("create_user failed");

    // get
    let fetched = crate::database::user::get_user_by_name(&uname)
        .await
        .expect("get_user_by_name failed");
    assert!(fetched.is_some());

    // list
    let _ = crate::database::user::list_users()
        .await
        .expect("list_users failed");

    // update
    let ok = crate::database::user::update_user_email(&uname, "new@test.local")
        .await
        .expect("update_user_email failed");
    assert!(ok);

    // delete
    let ok = crate::database::user::delete_user(&uname)
        .await
        .expect("delete_user failed");
    assert!(ok);

    // ensure gone
    let fetched = crate::database::user::get_user_by_name(&uname)
        .await
        .expect("get_user_by_name failed");
    assert!(fetched.is_none());
}
