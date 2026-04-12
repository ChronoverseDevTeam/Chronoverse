use serde_json::json;

use crate::crv2::ChronoverseApp;

use super::HiveResponse;

/// Handle `submit` RPC.
///
/// Finalises a pending submit: validates all chunks exist in the CAS store,
/// creates the changelist + file revisions in a single transaction, and
/// transitions the submit to `committed`.
pub async fn submit(app: &ChronoverseApp, submit_id: i64) -> HiveResponse {
    match app.submit(submit_id).await {
        Ok(result) => HiveResponse::ok(json!({
            "changelist_id": result.changelist_id,
        })),
        Err(e) => HiveResponse::err(e),
    }
}

/// Handle `cancel_submit` RPC.
///
/// Cancels a pending submit and releases all file locks.
pub async fn cancel_submit(app: &ChronoverseApp, submit_id: i64) -> HiveResponse {
    match app.cancel_submit(submit_id).await {
        Ok(()) => HiveResponse::ok(json!({})),
        Err(e) => HiveResponse::err(e),
    }
}
