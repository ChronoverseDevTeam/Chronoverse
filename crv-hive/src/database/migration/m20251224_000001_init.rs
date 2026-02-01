use sea_orm_migration::prelude::*;
use sea_orm::Statement;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 启用 ltree 扩展
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                "CREATE EXTENSION IF NOT EXISTS ltree".to_string(),
            ))
            .await?;

        // 创建 users 表
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Users::Id).string().not_null().primary_key())
                    .col(ColumnDef::new(Users::Password).string().not_null())
                    .to_owned(),
            )
            .await?;

        // 创建 files 表（使用 ltree 类型）
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    r#"
                    CREATE TABLE IF NOT EXISTS {} (
                        {} ltree NOT NULL PRIMARY KEY,
                        {} bigint NOT NULL,
                        {} jsonb NOT NULL
                    )
                    "#,
                    Files::Table.to_string(),
                    Files::Path.to_string(),
                    Files::CreatedAt.to_string(),
                    Files::Metadata.to_string(),
                ),
            ))
            .await?;

        // 强制 `files.path` 使用约定的 hex 编码格式（见 `crate::database::ltree_key`），
        // 避免误把原始 depot path（含 `/`、`.`、中文等）直接写入 ltree。
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    r#"ALTER TABLE {} ADD CONSTRAINT ck_files_path_hex CHECK (({}::text) ~ '^[0-9a-f]+(\.[0-9a-f]+)*$')"#,
                    Files::Table.to_string(),
                    Files::Path.to_string(),
                ),
            ))
            .await?;

        // 为 files.path 创建 GIST 索引以支持 ltree 查询
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    "CREATE INDEX IF NOT EXISTS idx_files_path_gist ON {} USING GIST ({})",
                    Files::Table.to_string(),
                    Files::Path.to_string(),
                ),
            ))
            .await?;

        // 创建 changelists 表
        manager
            .create_table(
                Table::create()
                    .table(Changelists::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Changelists::Id)
                            // Postgres 无 unsigned bigint；同时 proto / 业务逻辑使用 int64。
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Changelists::Author).string().not_null())
                    .col(ColumnDef::new(Changelists::Description).text().not_null())
                    .col(ColumnDef::new(Changelists::CommittedAt).big_integer().not_null())
                    .col(ColumnDef::new(Changelists::Metadata).json_binary().not_null())
                    .to_owned(),
            )
            .await?;

        // 创建 file_revisions 表（使用 ltree 类型）
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    r#"
                    CREATE TABLE IF NOT EXISTS {} (
                        {} ltree NOT NULL,
                        {} bigint NOT NULL,
                        {} bigint NOT NULL,
                        {} bigint NOT NULL,
                        {} jsonb NOT NULL,
                        {} bigint NOT NULL,
                        {} boolean NOT NULL,
                        {} bigint NOT NULL,
                        {} jsonb NOT NULL,
                        PRIMARY KEY ({}, {}, {}),
                        CONSTRAINT fk_file_revisions_path 
                            FOREIGN KEY ({}) 
                            REFERENCES {} ({}) 
                            ON DELETE CASCADE 
                            ON UPDATE CASCADE
                        ,
                        CONSTRAINT fk_file_revisions_changelist
                            FOREIGN KEY ({})
                            REFERENCES {} ({})
                            ON DELETE RESTRICT
                            ON UPDATE CASCADE
                    )
                    "#,
                    FileRevisions::Table.to_string(),
                    FileRevisions::Path.to_string(),
                    FileRevisions::Generation.to_string(),
                    FileRevisions::Revision.to_string(),
                    FileRevisions::ChangelistId.to_string(),
                    FileRevisions::BinaryId.to_string(),
                    FileRevisions::Size.to_string(),
                    FileRevisions::IsDelete.to_string(),
                    FileRevisions::CreatedAt.to_string(),
                    FileRevisions::Metadata.to_string(),
                    FileRevisions::Path.to_string(),
                    FileRevisions::Generation.to_string(),
                    FileRevisions::Revision.to_string(),
                    FileRevisions::Path.to_string(),
                    Files::Table.to_string(),
                    Files::Path.to_string(),
                    FileRevisions::ChangelistId.to_string(),
                    Changelists::Table.to_string(),
                    Changelists::Id.to_string(),
                ),
            ))
            .await?;

        // 强制 `file_revisions.path` 使用约定的 hex 编码格式（见 `crate::database::ltree_key`）。
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    r#"ALTER TABLE {} ADD CONSTRAINT ck_file_revisions_path_hex CHECK (({}::text) ~ '^[0-9a-f]+(\.[0-9a-f]+)*$')"#,
                    FileRevisions::Table.to_string(),
                    FileRevisions::Path.to_string(),
                ),
            ))
            .await?;

        // 为 file_revisions.path 创建 GIST 索引以支持 ltree 查询
        manager
            .get_connection()
            .execute(Statement::from_string(
                manager.get_database_backend(),
                format!(
                    "CREATE INDEX IF NOT EXISTS idx_file_revisions_path_gist ON {} USING GIST ({})",
                    FileRevisions::Table.to_string(),
                    FileRevisions::Path.to_string(),
                ),
            ))
            .await?;

        // 为 file_revisions.changelist_id 创建索引
        manager
            .create_index(
                Index::create()
                    .name("idx_file_revisions_changelist_id")
                    .table(FileRevisions::Table)
                    .col(FileRevisions::ChangelistId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(FileRevisions::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Changelists::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Files::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Users::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Password,
}

#[derive(DeriveIden)]
enum Files {
    Table,
    Path,
    CreatedAt,
    Metadata,
}

#[derive(DeriveIden)]
enum Changelists {
    Table,
    Id,
    Author,
    Description,
    CommittedAt,
    Metadata,
}

#[derive(DeriveIden)]
enum FileRevisions {
    Table,
    Path,
    Generation,
    Revision,
    ChangelistId,
    BinaryId,
    Size,
    IsDelete,
    CreatedAt,
    Metadata,
}
