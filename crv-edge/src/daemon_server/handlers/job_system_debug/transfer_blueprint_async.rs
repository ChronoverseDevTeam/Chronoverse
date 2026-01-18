use crate::daemon_server::error::{AppError, AppResult};
use crate::daemon_server::job::{
    JobEvent, JobRetentionPolicy, JobStatus, MessageStoragePolicy, WorkerProtocol,
};
use crate::daemon_server::state::AppState;
use crate::pb::{
    TransferBlueprintAsyncCheckReq, TransferBlueprintAsyncCheckRsp, TransferBlueprintAsyncStartReq,
    TransferBlueprintAsyncStartRsp, TransferBlueprintRsp,
};
use prost::Message;
use rand::Rng;
use std::sync::Arc;
use tokio::time::{Duration, sleep};
use tonic::{Request, Response, Status};

pub async fn start(
    state: AppState,
    req: Request<TransferBlueprintAsyncStartReq>,
) -> AppResult<Response<TransferBlueprintAsyncStartRsp>> {
    let req = req.into_inner();
    // Enable RingBuffer with capacity 100
    // Retain job for 60 seconds after completion
    let job = state.job_manager.create_job(
        None,
        MessageStoragePolicy::RingBuffer(100),
        WorkerProtocol::And,
        JobRetentionPolicy::Retain(60),
    );
    let job_id = job.data.read().unwrap().id.clone();

    let worker_count = req.worker_count.max(1);

    for i in 0..worker_count {
        let job_ref = job.clone();
        job.add_worker(async move { worker_task(job_ref, i).await });
    }

    job.start();

    Ok(Response::new(TransferBlueprintAsyncStartRsp { job_id }))
}

pub async fn check(
    state: AppState,
    req: Request<TransferBlueprintAsyncCheckReq>,
) -> AppResult<Response<TransferBlueprintAsyncCheckRsp>> {
    let req = req.into_inner();
    let job = state
        .job_manager
        .get_job(&req.job_id)
        .ok_or(AppError::Raw(Status::not_found("Job not found")))?;

    let status = {
        let data = job.data.read().unwrap();
        match &data.status {
            JobStatus::Pending | JobStatus::Running => "running".to_string(),
            JobStatus::Completed => "completed".to_string(),
            JobStatus::Failed(e) => {
                return Err(AppError::Raw(Status::internal(format!(
                    "Job Failed: {}",
                    e
                ))));
            }
            JobStatus::Cancelled => return Err(AppError::Raw(Status::cancelled("Job Cancelled"))),
        }
    };

    let events = job.consume_buffered_events();
    let messages: Vec<TransferBlueprintRsp> = events
        .into_iter()
        .filter_map(|e| match e {
            JobEvent::Payload(any) => TransferBlueprintRsp::decode(&any.value[..]).ok(),
            _ => None,
        })
        .collect();

    Ok(Response::new(TransferBlueprintAsyncCheckRsp {
        job_status: status,
        messages,
    }))
}

// Reuse worker_task logic (duplicated for simplicity or can be shared in utils)
async fn worker_task(
    job: Arc<crate::daemon_server::job::Job>,
    worker_id: i32,
) -> Result<(), String> {
    let total_chunks = 10;

    for i in 0..total_chunks {
        let delay = rand::thread_rng().gen_range(100..500);
        sleep(Duration::from_millis(delay)).await;

        let mut network_issue = rand::thread_rng().gen_bool(0.1);
        let mut retry = 0;
        while network_issue {
            retry += 1;
            if retry > 3 {
                // Fail after 3 retries
                return Err("Network timeout after 3 retries".to_string());
            }
            job.report_payload(TransferBlueprintRsp {
                worker_id: worker_id.to_string(),
                r#type: "warning".to_string(),
                message: format!("Network unstable, retrying... ({}/3)", retry),
                retry_count: retry,
            });
            sleep(Duration::from_millis(200)).await;
            network_issue = rand::thread_rng().gen_bool(0.01);
        }

        job.report_payload(TransferBlueprintRsp {
            worker_id: worker_id.to_string(),
            r#type: "progress".to_string(),
            message: format!("Chunk {}/{}", i + 1, total_chunks),
            retry_count: 0,
        });
    }

    Ok(())
}
