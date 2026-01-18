use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::state::AppState;
use crate::pb::{CancelJobReq, CancelJobRsp};
use tonic::{Request, Response};

pub async fn handle(
    state: AppState,
    req: Request<CancelJobReq>,
) -> AppResult<Response<CancelJobRsp>> {
    let req = req.into_inner();

    if let Some(job) = state.job_manager.get_job(&req.job_id) {
        job.cancel();
    } else {
        // Job not found, possibly already cleaned up. We can return success or error.
        // Returning success is usually idempotent and safer.
        // Or return NotFound error.
        return Err(AppError::NotFound(format!("Job not found: {}", req.job_id)));
    }

    Ok(Response::new(CancelJobRsp {}))
}
