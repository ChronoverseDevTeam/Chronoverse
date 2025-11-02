use futures::stream::StreamExt;
use mongodb::bson::doc;
use mongodb::bson;

use crate::database::get_mongo;

pub use crv_core::metadata::changelist::Changelist;

const COLLECTION_NAME: &str = "changelists";

fn collection() -> mongodb::Collection<Changelist> {
    get_mongo().collection::<Changelist>(COLLECTION_NAME)
}

/// 创建新的变更列表
pub async fn create_changelist(entity: Changelist) -> Result<(), mongodb::error::Error> {
    let coll = collection();
    coll.insert_one(entity).await?;
    Ok(())
}

/// 根据 ID 获取变更列表
pub async fn get_changelist_by_id(id: u64) -> Result<Option<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": id as i64};
    let found = coll.find_one(filter).await?;
    Ok(found)
}

/// 根据 workspace 名称获取所有变更列表
pub async fn list_changelists_by_workspace(workspace_name: &str) -> Result<Vec<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"workspace_name": workspace_name};
    let mut cursor = coll.find(filter).await?;
    let mut items = Vec::new();
    while let Some(res) = cursor.next().await {
        items.push(res?);
    }
    Ok(items)
}

/// 根据 owner 获取所有变更列表
pub async fn list_changelists_by_owner(owner: &str) -> Result<Vec<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"owner": owner};
    let mut cursor = coll.find(filter).await?;
    let mut items = Vec::new();
    while let Some(res) = cursor.next().await {
        items.push(res?);
    }
    Ok(items)
}

/// 获取特定 workspace 和 owner 的所有未提交变更列表
pub async fn list_pending_changelists(
    workspace_name: &str,
    owner: &str,
) -> Result<Vec<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {
        "workspace_name": workspace_name,
        "owner": owner,
        "submitted_at": { "$exists": false }
    };
    let mut cursor = coll.find(filter).await?;
    let mut items = Vec::new();
    while let Some(res) = cursor.next().await {
        items.push(res?);
    }
    Ok(items)
}

/// 获取特定 workspace 和 owner 的默认变更列表（ID 为 0）
pub async fn get_default_changelist(
    workspace_name: &str,
    owner: &str,
) -> Result<Option<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {
        "_id": 0i64,
        "workspace_name": workspace_name,
        "owner": owner
    };
    let found = coll.find_one(filter).await?;
    Ok(found)
}

/// 获取所有已提交的变更列表
pub async fn list_submitted_changelists(workspace_name: &str) -> Result<Vec<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {
        "workspace_name": workspace_name,
        "submitted_at": { "$exists": true }
    };
    let mut cursor = coll.find(filter).await?;
    let mut items = Vec::new();
    while let Some(res) = cursor.next().await {
        items.push(res?);
    }
    Ok(items)
}

/// 完全替换变更列表（用于更新文件列表等）
pub async fn replace_changelist(entity: Changelist) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": entity.id as i64};
    let result = coll.replace_one(filter, entity).await?;
    Ok(result.matched_count > 0)
}

/// 更新变更列表的描述
pub async fn update_changelist_description(
    id: u64,
    description: &str,
) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": id as i64};
    let update = doc! {
        "$set": {
            "description": description
        }
    };
    let res = coll.update_one(filter, update).await?;
    Ok(res.matched_count > 0)
}

/// 标记变更列表为已提交
pub async fn submit_changelist(id: u64) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {"_id": id as i64};
    let update = doc! {
        "$set": {
            "submitted_at": bson::DateTime::now()
        }
    };
    let res = coll.update_one(filter, update).await?;
    Ok(res.matched_count > 0)
}

/// 删除变更列表
pub async fn delete_changelist(id: u64) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let res = coll.delete_one(doc! {"_id": id as i64}).await?;
    Ok(res.deleted_count > 0)
}

/// 获取下一个可用的变更列表 ID（简单的自增实现）
pub async fn get_next_changelist_id() -> Result<u64, mongodb::error::Error> {
    let coll = collection();
    use mongodb::options::FindOptions;
    
    let options = FindOptions::builder()
        .sort(doc! {"_id": -1})
        .limit(1)
        .build();
    
    let mut cursor = coll.find(doc! {}).with_options(options).await?;
    
    if let Some(res) = cursor.next().await {
        let last = res?;
        Ok(last.id + 1)
    } else {
        // 如果没有任何变更列表，从 1 开始（0 预留给 default changelist）
        Ok(1)
    }
}

/// 列出所有变更列表（带过滤器）
pub async fn list_changelists_filtered(
    workspace_name: Option<&str>,
    owner: Option<&str>,
    submitted: Option<bool>,
) -> Result<Vec<Changelist>, mongodb::error::Error> {
    let coll = collection();
    let mut filter = doc! {};
    
    if let Some(ws) = workspace_name.and_then(|s| if s.trim().is_empty() { None } else { Some(s) }) {
        filter.insert("workspace_name", ws);
    }
    
    if let Some(o) = owner.and_then(|s| if s.trim().is_empty() { None } else { Some(s) }) {
        filter.insert("owner", o);
    }
    
    if let Some(is_submitted) = submitted {
        if is_submitted {
            filter.insert("submitted_at", doc! { "$exists": true });
        } else {
            filter.insert("submitted_at", doc! { "$exists": false });
        }
    }
    
    let mut cursor = coll.find(filter).await?;
    let mut items = Vec::new();
    while let Some(res) = cursor.next().await {
        items.push(res?);
    }
    Ok(items)
}

/// 确保默认变更列表存在，如果不存在则创建
pub async fn ensure_default_changelist(
    workspace_name: &str,
    owner: &str,
) -> Result<Changelist, mongodb::error::Error> {
    if let Some(cl) = get_default_changelist(workspace_name, owner).await? {
        Ok(cl)
    } else {
        let default_cl = Changelist::new_default(owner.to_string(), workspace_name.to_string());
        create_changelist(default_cl.clone()).await?;
        Ok(default_cl)
    }
}

/// 向默认变更列表添加文件（使用原子操作）
pub async fn add_files_to_default_changelist(
    workspace_name: &str,
    owner: &str,
    files: Vec<(String, crv_core::metadata::file_revision::MetaFileRevision)>,
) -> Result<(), mongodb::error::Error> {
    use mongodb::bson;
    
    // 先确保默认变更列表存在
    ensure_default_changelist(workspace_name, owner).await?;
    
    // 构建更新文档
    let coll = collection();
    let filter = doc! {
        "_id": 0i64,
        "workspace_name": workspace_name,
        "owner": owner
    };
    
    // 将文件转换为 BSON 文档
    let mut updates = doc! {};
    for (path, revision) in files {
        let revision_bson = bson::to_bson(&revision)
            .map_err(|e| mongodb::error::Error::custom(e))?;
        updates.insert(format!("files.{}", path), revision_bson);
    }
    
    let update = doc! { "$set": updates };
    coll.update_one(filter, update).await?;
    Ok(())
}

/// 从默认变更列表移除文件（使用原子操作）
pub async fn remove_files_from_default_changelist(
    workspace_name: &str,
    owner: &str,
    depot_paths: Vec<String>,
) -> Result<(), mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {
        "_id": 0i64,
        "workspace_name": workspace_name,
        "owner": owner
    };
    
    // 构建 $unset 操作
    let mut unset_doc = doc! {};
    for path in depot_paths {
        unset_doc.insert(format!("files.{}", path), "");
    }
    
    let update = doc! { "$unset": unset_doc };
    coll.update_one(filter, update).await?;
    Ok(())
}

/// 清空默认变更列表中的所有文件
pub async fn clear_default_changelist(
    workspace_name: &str,
    owner: &str,
) -> Result<(), mongodb::error::Error> {
    let coll = collection();
    let filter = doc! {
        "_id": 0i64,
        "workspace_name": workspace_name,
        "owner": owner
    };
    
    let update = doc! { "$set": { "files": {} } };
    coll.update_one(filter, update).await?;
    Ok(())
}

