use futures::stream::StreamExt;
use mongodb::bson::doc;

use crate::database::get_mongo;

pub use crv_core::user::entity::UserEntity;

const COLLECTION_NAME: &str = "users";

fn collection() -> mongodb::Collection<UserEntity> {
    get_mongo().collection::<UserEntity>(COLLECTION_NAME)
}

pub async fn ensure_indexes() -> Result<(), mongodb::error::Error> {
    Ok(())
}

pub async fn create_user(user: UserEntity) -> Result<(), mongodb::error::Error> {
    let coll = collection();
    coll.insert_one(user).await?;
    Ok(())
}

pub async fn get_user_by_name(name: &str) -> Result<Option<UserEntity>, mongodb::error::Error> {
    let coll = collection();
    let found = coll.find_one(doc! {"_id": name}).await?;
    Ok(found)
}

pub async fn list_users() -> Result<Vec<UserEntity>, mongodb::error::Error> {
    let coll = collection();
    let mut cursor = coll.find(doc! {}).await?;
    let mut v = Vec::new();
    while let Some(item) = cursor.next().await {
        v.push(item?);
    }
    Ok(v)
}

pub async fn update_user_email(name: &str, email: &str) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let res = coll
        .update_one(doc! {"_id": name}, doc! {"$set": {"email": email}})
        .await?;
    Ok(res.matched_count > 0)
}

pub async fn delete_user(name: &str) -> Result<bool, mongodb::error::Error> {
    let coll = collection();
    let res = coll.delete_one(doc! {"_id": name}).await?;
    Ok(res.deleted_count > 0)
}
