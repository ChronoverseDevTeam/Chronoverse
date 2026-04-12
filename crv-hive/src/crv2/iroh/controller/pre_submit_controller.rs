use serde::Deserialize;
use serde_json::json;

use crate::crv2::ChronoverseApp;

use super::HiveResponse;

/// Payload for a single file in a pre-submit request.
#[derive(Debug, Deserialize)]
pub struct PreSubmitFile {
    pub path: String,
    /// add | edit | delete
    pub action: String,
    pub chunk_hashes: Vec<String>,
    pub size: i64,
}

/// Handle `pre_submit` RPC.
///
/// Creates a pending submit, acquires pessimistic locks on all listed files,
/// and returns the submit ID. The caller must later call `submit` or
/// `cancel_submit` to finalise or release the locks.
pub async fn pre_submit(
    app: &ChronoverseApp,
    description: String,
    files: Vec<PreSubmitFile>,
) -> HiveResponse {
    match app.pre_submit(description, files).await {
        Ok(result) => HiveResponse::ok(json!({
            "submit_id": result.submit_id,
            "expires_at": result.expires_at,
        })),
        Err(e) => HiveResponse::err(e),
    }
}
