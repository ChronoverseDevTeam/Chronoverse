use std::vec;

use crate::{hive_server::submit::submit_service, pb::{SubmitReq, SubmitRsp, UploadFileChunkRsp}};
use serde_json::ser;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub type UploadFileChunkStream = ReceiverStream<Result<UploadFileChunkRsp, Status>>;

pub fn submit(
    r: Request<SubmitReq>,
) -> Result<Response<SubmitRsp>, Status> {
    let request = r.into_inner();

    let service = submit_service();

    let rsp = SubmitRsp {
        success: false,
        changelist_id: 0,
        committed_at: 0,
        conflicts: vec![],
        missing_chunks: vec![],
        latest_revisions: vec![],
        message: "not implemented".to_string(),
    };
    Ok(Response::new(rsp))
}