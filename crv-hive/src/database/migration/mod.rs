use sea_orm_migration::prelude::*;

mod m20251224_000001_init;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20251224_000001_init::Migration)]
    }
}


