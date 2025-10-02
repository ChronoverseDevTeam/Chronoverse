use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use futures::TryStreamExt;
use mongodb::bson::doc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalToken {
    #[serde(rename = "_id")]
    pub id: String,
    pub user: String,
    pub name: String,
    pub token_sha256: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub last_used_at: Option<DateTime<Utc>>,
}

const COLLECTION_NAME: &str = "personal_tokens";

fn collection() -> mongodb::Collection<PersonalToken> {
    crate::database::get_mongo().collection::<PersonalToken>(COLLECTION_NAME)
}

pub async fn insert(token: PersonalToken) -> Result<(), mongodb::error::Error> {
    collection().insert_one(token).await?;
    Ok(())
}

pub async fn list_by_user(user: &str) -> Result<Vec<PersonalToken>, mongodb::error::Error> {
    let mut cursor = collection().find(doc! {"user": user}).await?;
    let mut v = Vec::new();
    while let Some(item) = cursor.try_next().await? {
        v.push(item);
    }
    Ok(v)
}

pub async fn delete_by_id(user: &str, id: &str) -> Result<bool, mongodb::error::Error> {
    let res = collection().delete_one(doc! {"_id": id, "user": user}).await?;
    Ok(res.deleted_count > 0)
}

pub async fn find_by_sha(sha: &str) -> Result<Option<PersonalToken>, mongodb::error::Error> {
    let found = collection().find_one(doc! {"token_sha256": sha}).await?;
    Ok(found)
}

pub async fn touch_last_used(id: &str) -> Result<(), mongodb::error::Error> {
    let _ = collection().update_one(doc! {"_id": id}, doc! {"$set": {"last_used_at": Utc::now()} }).await?;
    Ok(())
}


