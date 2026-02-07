use tonic::{Request, Response, Status};

use crate::common::depot_path::DepotPath;
use crate::database::service as db_service;
use crate::logging::HiveLog;
use crate::pb::{FileRevision as PbFileRevision, GetFileTreeReq, GetFileTreeRsp};

pub async fn get_file_tree(
    log: HiveLog,
    request: Request<GetFileTreeReq>,
) -> Result<Response<GetFileTreeRsp>, Status> {
    let _g = log.enter();
    let req = request.into_inner();

    let depot = DepotPath::parse(&req.depot_wildcard).map_err(|e| {
        Status::invalid_argument(format!(
            "invalid depot_wildcard '{}': {}",
            req.depot_wildcard, e
        ))
    })?;

    log.info(&format!(
        "get_file_tree: path={}, changelist_id={}",
        depot, req.changelist_id
    ));

    let models = db_service::get_file_tree_revisions(&depot, req.changelist_id)
        .await
        .map_err(|e| Status::internal(format!("database error while get_file_tree: {e}")))?;

    let mut file_revisions = Vec::with_capacity(models.len());
    for m in models {
        let path = m.to_depot_path_string().map_err(|e| {
            Status::internal(format!("failed to decode ltree path: {e}"))
        })?;

        let binary_id = m
            .binary_id
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        file_revisions.push(PbFileRevision {
            path,
            generation: m.generation,
            revision: m.revision,
            changelist_id: m.changelist_id,
            binary_id,
            size: m.size,
            revision_created_at: m.created_at,
        });
    }

    Ok(Response::new(GetFileTreeRsp { file_revisions }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    use std::sync::OnceLock;

    static INIT_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

    fn should_run_hive_db_tests() -> bool {
        if std::env::var("CRV_SKIP_HIVE_DB_TESTS").as_deref() == Ok("1") {
            eprintln!("skip get_file_tree db tests (CRV_SKIP_HIVE_DB_TESTS=1)");
            return false;
        }

        if std::env::var("CRV_RUN_HIVE_DB_TESTS").as_deref() == Ok("1") {
            return true;
        }

        eprintln!("skip get_file_tree db tests (set CRV_RUN_HIVE_DB_TESTS=1 and run with --ignored)");
        false
    }

    fn test_pg_config() -> crate::config::entity::ConfigEntity {
        let host = std::env::var("CRV_HIVE_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("CRV_HIVE_TEST_PG_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(5432);
        let db = std::env::var("CRV_HIVE_TEST_PG_DB").unwrap_or_else(|_| "chronoverse".into());
        let user = std::env::var("CRV_HIVE_TEST_PG_USER").unwrap_or_else(|_| "postgres".into());
        let pass = std::env::var("CRV_HIVE_TEST_PG_PASS").unwrap_or_else(|_| "postgres".into());

        let mut cfg = crate::config::entity::ConfigEntity::default();
        cfg.postgres_hostname = host;
        cfg.postgres_port = port;
        cfg.postgres_database = db;
        cfg.postgres_username = user;
        cfg.postgres_password = pass;
        cfg
    }

    async fn ensure_db() {
        if database::try_get().is_some() {
            return;
        }

        let m = INIT_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()));
        let _guard = m.lock().await;

        if database::try_get().is_some() {
            return;
        }

        let cfg = test_pg_config();
        let _ = crate::config::holder::try_set_config(cfg);
        if let Err(e) = database::init().await {
            if !e.to_string().contains("Database already initialized") {
                panic!("db init: {e}");
            }
        }
    }

    async fn insert_revision(
        depot_path: &str,
        generation: i64,
        revision: i64,
        is_delete: bool,
    ) -> i64 {
        ensure_db().await;
        let db = database::get();
        let backend = DatabaseBackend::Postgres;

        let has_changes = db
            .query_one(Statement::from_sql_and_values(
                backend,
                r#"
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = 'public'
                  AND table_name = 'changelists'
                  AND column_name = 'changes'
                LIMIT 1
                "#,
                vec![],
            ))
            .await
            .expect("check changelists schema")
            .is_some();

        let cl_row = if has_changes {
            db.query_one(Statement::from_sql_and_values(
                backend,
                r#"
                INSERT INTO changelists (author, description, changes, committed_at, metadata)
                VALUES ($1, $2, '[]'::jsonb, $3, $4)
                RETURNING id
                "#,
                vec![
                    "test".into(),
                    "test".into(),
                    0i64.into(),
                    serde_json::json!({}).into(),
                ],
            ))
            .await
            .expect("insert changelist")
        } else {
            db.query_one(Statement::from_sql_and_values(
                backend,
                r#"
                INSERT INTO changelists (author, description, committed_at, metadata)
                VALUES ($1, $2, $3, $4)
                RETURNING id
                "#,
                vec![
                    "test".into(),
                    "test".into(),
                    0i64.into(),
                    serde_json::json!({}).into(),
                ],
            ))
            .await
            .expect("insert changelist")
        };
        let cl = cl_row.expect("changelist row");
        let cl_id: i64 = cl.try_get("", "id").expect("get changelist id");

        let key = crate::database::ltree_key::depot_path_str_to_ltree_key(depot_path)
            .expect("encode depot path to ltree key");

        db.execute(Statement::from_sql_and_values(
            backend,
            r#"
            INSERT INTO files (path, created_at, metadata)
            VALUES ($1::ltree, 0, '{}'::jsonb)
            ON CONFLICT DO NOTHING
            "#,
            vec![key.clone().into()],
        ))
        .await
        .expect("insert file");

        db.execute(Statement::from_sql_and_values(
            backend,
            r#"
            INSERT INTO file_revisions
                (path, generation, revision, changelist_id, binary_id, size, is_delete, created_at, metadata)
            VALUES
                ($1::ltree, $2, $3, $4, '[]'::jsonb, 0, $5, 0, '{}'::jsonb)
            "#,
            vec![
                key.into(),
                generation.into(),
                revision.into(),
                cl_id.into(),
                is_delete.into(),
            ],
        ))
        .await
        .expect("insert file revision");

        cl_id
    }

    fn unique_depot_dir(name: &str) -> String {
        format!("//tests/get_file_tree/{}/", uuid::Uuid::new_v4().to_string() + name)
    }

    async fn file_tree_file_with_changelist_cutoff() {
        let base = unique_depot_dir("file_cutoff");
        let path = format!("{base}a.txt");

        let cl1 = insert_revision(&path, 1, 1, false).await;
        let _cl2 = insert_revision(&path, 1, 2, false).await;

        let req = GetFileTreeReq {
            depot_wildcard: path.clone(),
            changelist_id: cl1,
        };
        let log = HiveLog::new("GetFileTree(test_file_cutoff)");
        let resp = get_file_tree(log, Request::new(req))
            .await
            .expect("get_file_tree");
        let out = resp.into_inner();
        assert_eq!(out.file_revisions.len(), 1);
        assert_eq!(out.file_revisions[0].revision, 1);

        let req_latest = GetFileTreeReq {
            depot_wildcard: path,
            changelist_id: 0,
        };
        let log = HiveLog::new("GetFileTree(test_file_latest)");
        let resp = get_file_tree(log, Request::new(req_latest))
            .await
            .expect("get_file_tree");
        let out = resp.into_inner();
        assert_eq!(out.file_revisions.len(), 1);
        assert_eq!(out.file_revisions[0].revision, 2);
    }

    async fn file_tree_directory_and_wildcard() {
        let base = unique_depot_dir("dir");
        let file_a = format!("{base}a.txt");
        let file_b = format!("{base}b.txt");
        let nested = format!("{base}sub/c.txt");

        insert_revision(&file_a, 1, 1, false).await;
        insert_revision(&file_b, 1, 1, false).await;
        insert_revision(&nested, 1, 1, false).await;

        let dir_req = GetFileTreeReq {
            depot_wildcard: base.clone(),
            changelist_id: 0,
        };
        let log = HiveLog::new("GetFileTree(test_dir)");
        let resp = get_file_tree(log, Request::new(dir_req))
            .await
            .expect("get_file_tree");
        let out = resp.into_inner();
        let paths: std::collections::HashSet<String> =
            out.file_revisions.into_iter().map(|r| r.path).collect();
        assert!(paths.contains(&file_a));
        assert!(paths.contains(&file_b));
        assert!(!paths.contains(&nested));

        let wildcard_req = GetFileTreeReq {
            depot_wildcard: format!("{}...", base),
            changelist_id: 0,
        };
        let log = HiveLog::new("GetFileTree(test_wildcard)");
        let resp = get_file_tree(log, Request::new(wildcard_req))
            .await
            .expect("get_file_tree");
        let out = resp.into_inner();
        let paths: std::collections::HashSet<String> =
            out.file_revisions.into_iter().map(|r| r.path).collect();
        assert!(paths.contains(&file_a));
        assert!(paths.contains(&file_b));
        assert!(paths.contains(&nested));
    }

    #[tokio::test]
    async fn test_invalid_depot_wildcard() {
        let req = GetFileTreeReq {
            depot_wildcard: "invalid".to_string(),
            changelist_id: 0,
        };
        let log = HiveLog::new("GetFileTree(test_invalid)");
        let err = get_file_tree(log, Request::new(req))
            .await
            .expect_err("should reject invalid depot_wildcard");
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    #[ignore = "requires external Postgres; enable with CRV_RUN_HIVE_DB_TESTS=1"]
    fn get_file_tree_tests_harness() {
        if !should_run_hive_db_tests() {
            return;
        }

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .expect("build tokio runtime for get_file_tree tests");

        rt.block_on(async {
            file_tree_file_with_changelist_cutoff().await;
            file_tree_directory_and_wildcard().await;
        });
    }
}

