use serde_json::json;

use crate::crv2::ChronoverseApp;

use super::HiveResponse;

pub async fn browse_depot_tree(
    app: &ChronoverseApp,
    path: String,
    changelist_id: i64,
    recursive: bool,
) -> HiveResponse {
    match app
        .browse_depot_tree(&path, changelist_id, recursive)
        .await
    {
        Ok(result) => HiveResponse::ok(json!(result)),
        Err(error) => HiveResponse::err(error),
    }
}

pub async fn query_path_history(
    app: &ChronoverseApp,
    path: String,
    from_changelist: Option<i64>,
    to_changelist: Option<i64>,
    limit: Option<usize>,
) -> HiveResponse {
    match app
        .query_path_history(&path, from_changelist, to_changelist, limit)
        .await
    {
        Ok(result) => HiveResponse::ok(json!(result)),
        Err(error) => HiveResponse::err(error),
    }
}