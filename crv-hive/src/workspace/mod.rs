use futures::stream::StreamExt;
use mongodb::bson::doc;
use mongodb::bson;

use crate::database::get_mongo;

pub mod entity;
pub use entity::WorkspaceEntity;

const COLLECTION_NAME: &str = "workspaces";

fn collection() -> mongodb::Collection<WorkspaceEntity> {
    get_mongo().collection::<WorkspaceEntity>(COLLECTION_NAME)
}

pub async fn ensure_indexes() -> Result<(), mongodb::error::Error> { Ok(()) }

pub async fn create_workspace(entity: WorkspaceEntity) -> Result<(), mongodb::error::Error> {
    let coll = collection();
    coll.insert_one(entity).await?;
    Ok(())
}

pub async fn get_workspace_by_name(name: &str) -> Result<Option<WorkspaceEntity>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": name};
    let found = coll.find_one(filter).await?;
    Ok(found)
}

pub async fn list_workspaces() -> Result<Vec<WorkspaceEntity>, mongodb::error::Error> {
    let coll = collection();
    let mut cursor = coll.find(doc! {}).await?;
    let mut items = Vec::new();
    while let Some(res) = cursor.next().await {
        items.push(res?);
    }
    Ok(items)
}

pub async fn update_workspace_path(name: &str, new_path: &str) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": name};
    let update = doc! {
        "$set": {
            "path": new_path,
            "updated_at": bson::DateTime::now()
        }
    };
    let res = coll.update_one(filter, update).await?;
    Ok(res.matched_count > 0)
}

pub async fn delete_workspace(name: &str) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let res = coll.delete_one(doc! {"_id": name}).await?;
    Ok(res.deleted_count > 0)
}


