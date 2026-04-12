//! Listens to iroh-blobs provider events and extends the expiry of pending
//! submits whenever push activity is observed.
//!
//! When a push request arrives, we extract the blob hash from the event,
//! look up which pending submit owns that hash via the in-memory
//! [`SubmitRegistry`], and extend only that submit's expiry.
//!
//! Per-submit throttling ensures at most one DB write per submit per
//! half-lock-duration (~5 s with a 10 s lock), regardless of how many
//! chunks are being pushed.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh_blobs::provider::events::{EventMask, EventSender, ProviderMessage, RequestMode};
use tokio::sync::mpsc;

use crate::crv2::postgres::dao;
use crate::crv2::postgres::executor::PostgreExecutor;
use crate::crv2::service::submit::lock_duration_ms;
use crate::crv2::service::submit_registry::SubmitRegistry;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Create an [`EventSender`] configured to receive push-request notifications
/// and the corresponding receiver.
///
/// Uses `Notify` mode (one event per push request, not per-chunk progress)
/// to keep event volume manageable.
pub fn create_event_channel() -> (EventSender, mpsc::Receiver<ProviderMessage>) {
    let mask = EventMask {
        push: RequestMode::Notify,
        ..EventMask::DEFAULT
    };
    EventSender::channel(64, mask)
}

/// Spawn a background task that listens for iroh-blobs push events and
/// extends the expiry of the corresponding pending submit.
pub fn spawn_expiry_extender(
    pg: Arc<PostgreExecutor>,
    registry: Arc<SubmitRegistry>,
    mut rx: mpsc::Receiver<ProviderMessage>,
) -> tokio::task::JoinHandle<()> {
    let throttle = Duration::from_millis((lock_duration_ms() / 2) as u64);

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let hash = match extract_push_hash(&msg) {
                Some(h) => h,
                None => continue,
            };

            // Look up which submit(s) declared this hash.
            let submit_ids = registry.lookup(&hash);
            if submit_ids.is_empty() {
                continue;
            }

            let new_expires = now_ms() + lock_duration_ms();

            for submit_id in submit_ids {
                // Per-submit throttle: skip if recently extended.
                if !registry.try_mark_extended(submit_id, throttle) {
                    continue;
                }

                if let Err(e) =
                    dao::submit::extend_expiry(pg.connection(), submit_id, new_expires).await
                {
                    tracing::warn!(submit_id, "failed to extend submit expiry: {e}");
                }
            }
        }

        tracing::debug!("blob event listener shut down");
    })
}

/// Extract the blob [`Hash`] from a push-request event.
fn extract_push_hash(msg: &ProviderMessage) -> Option<iroh_blobs::Hash> {
    match msg {
        ProviderMessage::PushRequestReceivedNotify(msg) => {
            Some(msg.inner.request.hash)
        }
        ProviderMessage::PushRequestReceived(msg) => {
            Some(msg.inner.request.hash)
        }
        _ => None,
    }
}
