use sea_orm_migration::prelude::*;

mod m20251224_000001_init;
mod m20251229_000002_add_seen_on_branches_to_files;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251224_000001_init::Migration),
            Box::new(m20251229_000002_add_seen_on_branches_to_files::Migration),
        ]
    }
}


