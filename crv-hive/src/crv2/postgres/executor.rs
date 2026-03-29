use std::{future::Future, pin::Pin};

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DatabaseTransaction, DbErr,
    ExecResult, QueryResult, Statement, TransactionError, TransactionTrait,
};
use thiserror::Error;

use crate::crv2::config::DatabaseConfig;

use super::init;

pub type TransactionFuture<'a, T, E> =
    Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;

#[derive(Debug)]
pub struct PostgreExecutor {
    pool: DatabaseConnection,
}

#[derive(Debug, Error)]
pub enum PostgreExecutorError {
    #[error("postgres connection error: {0}")]
    Db(#[from] DbErr),
    #[error("postgres initialization error: {0}")]
    Init(#[from] init::InitError),
}

impl PostgreExecutor {
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, PostgreExecutorError> {
        let mut options = ConnectOptions::new(config.url.clone());
        options.max_connections(config.max_connections);

        let pool = Database::connect(options).await?;
        Ok(Self { pool })
    }

    pub async fn connect_and_init(config: &DatabaseConfig) -> Result<Self, PostgreExecutorError> {
        let executor = Self::connect(config).await?;
        executor.initialize().await?;
        Ok(executor)
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.pool
    }

    pub async fn initialize(&self) -> Result<(), PostgreExecutorError> {
        init::init(&self.pool).await?;
        Ok(())
    }

    pub async fn execute_sql(&self, sql: impl Into<String>) -> Result<ExecResult, PostgreExecutorError> {
        let statement = Statement::from_string(self.pool.get_database_backend(), sql.into());
        Ok(self.pool.execute(statement).await?)
    }

    pub async fn query_one(
        &self,
        sql: impl Into<String>,
    ) -> Result<Option<QueryResult>, PostgreExecutorError> {
        let statement = Statement::from_string(self.pool.get_database_backend(), sql.into());
        Ok(self.pool.query_one(statement).await?)
    }

    pub async fn query_all(
        &self,
        sql: impl Into<String>,
    ) -> Result<Vec<QueryResult>, PostgreExecutorError> {
        let statement = Statement::from_string(self.pool.get_database_backend(), sql.into());
        Ok(self.pool.query_all(statement).await?)
    }

    pub async fn begin(&self) -> Result<DatabaseTransaction, PostgreExecutorError> {
        Ok(self.pool.begin().await?)
    }

    pub async fn transaction<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        T: Send,
        E: From<PostgreExecutorError> + std::error::Error + Send + 'static,
        F: for<'a> FnOnce(&'a DatabaseTransaction) -> TransactionFuture<'a, T, E> + Send,
    {
        self.pool
            .transaction(|txn| operation(txn))
            .await
            .map_err(|err| match err {
                TransactionError::Connection(db_err) => E::from(PostgreExecutorError::Db(db_err)),
                TransactionError::Transaction(exec_err) => exec_err,
            })
    }

    pub async fn close(&self) -> Result<(), PostgreExecutorError> {
        self.pool.clone().close().await?;
        Ok(())
    }
}