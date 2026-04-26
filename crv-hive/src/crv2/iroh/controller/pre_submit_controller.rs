use serde::Deserialize;
use serde_json::json;

use crate::crv2::{ChronoverseApp, service};

use super::HiveResponse;

/// Payload for a single file in a pre-submit request.
#[derive(Debug, Deserialize)]
pub struct PreSubmitFilePayload {
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
    files: Vec<PreSubmitFilePayload>,
) -> HiveResponse {
    let files = files
        .into_iter()
        .map(|file| service::PreSubmitFile {
            path: file.path,
            action: file.action,
            chunk_hashes: file.chunk_hashes,
            size: file.size,
        })
        .collect();

    match app.pre_submit(description, files).await {
        Ok(result) => HiveResponse::ok(json!({
            "submit_id": result.submit_id,
            "expires_at": result.expires_at,
        })),
        Err(e) => HiveResponse::err(e),
    }
}
