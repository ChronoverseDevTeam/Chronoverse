use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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

        manager
            .create_table(
                Table::create()
                    .table(Branches::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Branches::Id).string().not_null().primary_key())
                    .col(ColumnDef::new(Branches::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Branches::CreatedBy).string().not_null())
                    .col(ColumnDef::new(Branches::HeadChangelistId).big_integer().not_null())
                    .col(ColumnDef::new(Branches::Metadata).json_binary().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Changelists::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Changelists::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Changelists::ParentChangelistId).big_integer().not_null())
                    .col(ColumnDef::new(Changelists::BranchId).string().not_null())
                    .col(ColumnDef::new(Changelists::Author).string().not_null())
                    .col(ColumnDef::new(Changelists::Description).text().not_null())
                    .col(ColumnDef::new(Changelists::Changes).json_binary().not_null())
                    .col(ColumnDef::new(Changelists::CommittedAt).big_integer().not_null())
                    .col(ColumnDef::new(Changelists::FilesCount).big_integer().not_null())
                    .col(ColumnDef::new(Changelists::Metadata).json_binary().not_null())
                    .index(
                        Index::create()
                            .name("idx_changelists_branch_id")
                            .table(Changelists::Table)
                            .col(Changelists::BranchId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Files::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Files::Id).string().not_null().primary_key())
                    .col(ColumnDef::new(Files::Path).text().not_null())
                    .col(ColumnDef::new(Files::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(Files::Metadata).json_binary().not_null())
                    .index(
                        Index::create()
                            .name("idx_files_path")
                            .table(Files::Table)
                            .col(Files::Path),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(FileRevisions::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(FileRevisions::Id).string().not_null().primary_key())
                    .col(ColumnDef::new(FileRevisions::BranchId).string().not_null())
                    .col(ColumnDef::new(FileRevisions::FileId).string().not_null())
                    .col(ColumnDef::new(FileRevisions::ChangelistId).big_integer().not_null())
                    .col(ColumnDef::new(FileRevisions::BinaryId).json_binary().not_null())
                    .col(ColumnDef::new(FileRevisions::ParentRevisionId).string().not_null())
                    .col(ColumnDef::new(FileRevisions::Size).big_integer().not_null())
                    .col(ColumnDef::new(FileRevisions::IsDelete).boolean().not_null())
                    .col(ColumnDef::new(FileRevisions::CreatedAt).big_integer().not_null())
                    .col(ColumnDef::new(FileRevisions::Metadata).json_binary().not_null())
                    .index(
                        Index::create()
                            .name("uidx_file_revisions_branch_file_cl")
                            .table(FileRevisions::Table)
                            .col(FileRevisions::BranchId)
                            .col(FileRevisions::FileId)
                            .col(FileRevisions::ChangelistId)
                            .unique(),
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
            .drop_table(Table::drop().table(Files::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Changelists::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Branches::Table).if_exists().to_owned())
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
enum Branches {
    Table,
    Id,
    CreatedAt,
    CreatedBy,
    HeadChangelistId,
    Metadata,
}

#[derive(DeriveIden)]
enum Changelists {
    Table,
    Id,
    ParentChangelistId,
    BranchId,
    Author,
    Description,
    Changes,
    CommittedAt,
    FilesCount,
    Metadata,
}

#[derive(DeriveIden)]
enum Files {
    Table,
    Id,
    Path,
    CreatedAt,
    Metadata,
}

#[derive(DeriveIden)]
enum FileRevisions {
    Table,
    Id,
    BranchId,
    FileId,
    ChangelistId,
    BinaryId,
    ParentRevisionId,
    Size,
    IsDelete,
    CreatedAt,
    Metadata,
}


