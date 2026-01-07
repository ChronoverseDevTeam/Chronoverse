use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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

        // 创建 files 表
        manager
            .create_table(
                Table::create()
                    .table(Files::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Files::Path).string().not_null().primary_key())
                    .col(ColumnDef::new(Files::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Files::Metadata).json_binary().not_null())
                    .to_owned(),
            )
            .await?;

        // 创建 changelists 表
        manager
            .create_table(
                Table::create()
                    .table(Changelists::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Changelists::Id)
                            .big_unsigned()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Changelists::Author).string().not_null())
                    .col(ColumnDef::new(Changelists::Description).text().not_null())
                    .col(ColumnDef::new(Changelists::Changes).json_binary().not_null())
                    .col(ColumnDef::new(Changelists::CommittedAt).big_integer().not_null())
                    .col(ColumnDef::new(Changelists::Metadata).json_binary().not_null())
                    .to_owned(),
            )
            .await?;

        // 创建 file_revisions 表
        manager
            .create_table(
                Table::create()
                    .table(FileRevisions::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(FileRevisions::Path).string().not_null())
                    .col(ColumnDef::new(FileRevisions::Generation).big_unsigned().not_null())
                    .col(ColumnDef::new(FileRevisions::Revision).big_unsigned().not_null())
                    .col(ColumnDef::new(FileRevisions::ChangelistId).big_integer().not_null())
                    .col(ColumnDef::new(FileRevisions::BinaryId).json_binary().not_null())
                    .col(ColumnDef::new(FileRevisions::Size).big_unsigned().not_null())
                    .col(ColumnDef::new(FileRevisions::IsDelete).boolean().not_null())
                    .col(ColumnDef::new(FileRevisions::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(FileRevisions::Metadata).json_binary().not_null())
                    .primary_key(
                        Index::create()
                            .name("pk_file_revisions")
                            .col(FileRevisions::Path)
                            .col(FileRevisions::Generation)
                            .col(FileRevisions::Revision),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_file_revisions_path")
                            .from(FileRevisions::Table, FileRevisions::Path)
                            .to(Files::Table, Files::Path)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .index(
                        Index::create()
                            .name("idx_file_revisions_changelist_id")
                            .table(FileRevisions::Table)
                            .col(FileRevisions::ChangelistId),
                    )
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
    Changes,
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
