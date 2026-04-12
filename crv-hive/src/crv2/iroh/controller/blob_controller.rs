use serde_json::json;

use crate::crv2::ChronoverseApp;

use super::HiveResponse;

/// Handle `get_blob_ticket` RPC.
pub async fn get_blob_ticket(app: &ChronoverseApp, hash: String) -> HiveResponse {
    match app.create_blob_ticket(&hash).await {
        Ok(offer) => HiveResponse::ok(json!({
            "hash": offer.hash,
            "ticket": offer.ticket.to_string(),
            "format": "raw"
        })),
        Err(e) => HiveResponse::err(e),
    }
}
