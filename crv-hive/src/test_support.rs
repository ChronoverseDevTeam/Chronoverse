#[cfg(test)]
use std::future::Future;
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

#[cfg(test)]
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

#[cfg(test)]
static DB_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
#[cfg(test)]
static DB_TEST_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

#[cfg(test)]
pub fn should_run_hive_db_tests() -> bool {
    if std::env::var("CRV_SKIP_HIVE_DB_TESTS").as_deref() == Ok("1") {
        eprintln!("skip hive db tests (CRV_SKIP_HIVE_DB_TESTS=1)");
        return false;
    }

    if std::env::var("CRV_RUN_HIVE_DB_TESTS").as_deref() == Ok("1") {
        return true;
    }

    eprintln!("skip hive db tests (set CRV_RUN_HIVE_DB_TESTS=1 and run with --ignored)");
    false
}

#[cfg(test)]
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

#[cfg(test)]
pub async fn ensure_hive_db() {
    if crate::database::try_get().is_some() {
        return;
    }

    let cfg = test_pg_config();
    let _ = crate::config::holder::try_set_config(cfg);
    if let Err(e) = crate::database::init().await {
        if !e.to_string().contains("Database already initialized") {
            panic!("db init: {e}");
        }
    }
}

#[cfg(test)]
async fn changelists_has_changes_column(db: &DatabaseConnection) -> bool {
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
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
    .is_some()
}

#[cfg(test)]
pub async fn insert_test_changelist(db: &DatabaseConnection) -> i64 {
    let has_changes = changelists_has_changes_column(db).await;
    let backend = DatabaseBackend::Postgres;
    let row = if has_changes {
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

    let row = row.expect("changelist row");
    row.try_get("", "id").expect("get changelist id")
}

#[cfg(test)]
pub fn run_hive_db_test<F, Fut>(f: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    if !should_run_hive_db_tests() {
        return;
    }

    let _guard = DB_TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("lock hive db test mutex");

    let rt = DB_TEST_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(1)
            .build()
            .expect("build tokio runtime for hive db tests")
    });

    rt.block_on(async {
        ensure_hive_db().await;
        f().await;
    });
}

