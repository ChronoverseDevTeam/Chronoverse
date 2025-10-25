use futures::stream::StreamExt;
use mongodb::bson::doc;
use mongodb::bson;

use crate::database::get_mongo;

pub use crv_core::workspace::entity::WorkspaceEntity;

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

/// Upsert workspace: 如果存在则更新，不存在则创建
/// 返回 true 表示创建了新工作区，false 表示更新了已存在的工作区
pub async fn upsert_workspace(entity: WorkspaceEntity) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": &entity.name};
    
    // 使用 replace_one 替换整个文档，保留 created_at
    use mongodb::options::ReplaceOptions;
    
    // 先尝试获取现有文档的 created_at
    let existing = get_workspace_by_name(&entity.name).await?;
    
    let mut final_entity = entity;
    if let Some(existing_workspace) = existing {
        // 如果存在，保留原来的 created_at
        final_entity.created_at = existing_workspace.created_at;
    }
    
    let options = ReplaceOptions::builder().upsert(true).build();
    let result = coll.replace_one(filter, final_entity).with_options(options).await?;
    
    // upserted_id 存在表示是新创建的
    Ok(result.upserted_id.is_some())
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

pub async fn list_workspaces_filtered(
    name: Option<&str>,
    owner: Option<&str>,
    device_finger_print: Option<&str>,
) -> Result<Vec<WorkspaceEntity>, mongodb::error::Error> {
    let coll = collection();
    let mut filter = doc! {};
    if let Some(n) = name.and_then(|s| if s.trim().is_empty() { None } else { Some(s) }) {
        filter.insert("_id", n);
    }
    if let Some(o) = owner.and_then(|s| if s.trim().is_empty() { None } else { Some(s) }) {
        filter.insert("owner", o);
    }
    if let Some(d) = device_finger_print.and_then(|s| if s.trim().is_empty() { None } else { Some(s) }) {
        filter.insert("device_finger_print", d);
    }

    let mut cursor = coll.find(filter).await?;
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


