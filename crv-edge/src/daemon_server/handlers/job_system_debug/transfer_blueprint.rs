use crate::daemon_server::error::AppResult;
use crate::daemon_server::job::{
    JobEvent, JobRetentionPolicy, JobStatus, MessageStoragePolicy, WorkerProtocol,
};
use crate::daemon_server::state::AppState;
use crate::pb::{TransferBlueprintReq, TransferBlueprintRsp};
use prost::Message;
use rand::Rng;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};
use tokio::time::{Duration, sleep};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status};

pub type TransferBlueprintStream =
    Pin<Box<dyn Stream<Item = Result<TransferBlueprintRsp, Status>> + Send + 'static>>;

struct JobCancelOnDropStream {
    stream: TransferBlueprintStream,
    job: Weak<crate::daemon_server::job::Job>,
}

impl Stream for JobCancelOnDropStream {
    type Item = Result<TransferBlueprintRsp, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next(cx)
    }
}

impl Drop for JobCancelOnDropStream {
    fn drop(&mut self) {
        if let Some(job) = self.job.upgrade() {
            job.cancel();
        }
    }
}

pub async fn handle(
    state: AppState,
    req: Request<TransferBlueprintReq>,
) -> AppResult<Response<TransferBlueprintStream>> {
    let req = req.into_inner();
    let job = state.job_manager.create_job(
        None,
        MessageStoragePolicy::None,
        WorkerProtocol::And,
        JobRetentionPolicy::Immediate,
    );
    // 订阅消息队列
    let rx = job.tx.subscribe();

    let worker_count = req.worker_count.max(1);

    for i in 0..worker_count {
        let job_ref = job.clone();
        job.add_worker(async move { worker_task(job_ref, i).await });
    }

    job.clone().start();

    let output_stream = BroadcastStream::new(rx).filter_map(move |res| {
        match res {
            Ok(event) => match event {
                JobEvent::Payload(any) => {
                    // Attempt to decode as TransferBlueprintRsp
                    TransferBlueprintRsp::decode(&any.value[..]).ok().map(Ok)
                }
                JobEvent::Error(e) => Some(Err(Status::internal(e))),
                JobEvent::StatusChange(JobStatus::Completed) => None,
                JobEvent::StatusChange(JobStatus::Failed(e)) => Some(Err(Status::internal(e))),
                JobEvent::StatusChange(JobStatus::Cancelled) => {
                    Some(Err(Status::cancelled("Job Cancelled")))
                }
                _ => None,
            },
            Err(_) => Some(Err(Status::internal("Stream lagged"))),
        }
    });

    let wrapped_stream = JobCancelOnDropStream {
        stream: Box::pin(output_stream),
        job: Arc::downgrade(&job),
    };

    Ok(Response::new(
        Box::pin(wrapped_stream) as TransferBlueprintStream
    ))
}

async fn worker_task(
    job: Arc<crate::daemon_server::job::Job>,
    worker_id: i32,
) -> Result<(), String> {
    let total_chunks = 10;

    for i in 0..total_chunks {
        // Simulate work
        let delay = rand::thread_rng().gen_range(100..500);
        sleep(Duration::from_millis(delay)).await;

        // Simulate Network Fluctuation
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
            network_issue = rand::thread_rng().gen_bool(0.1);
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
