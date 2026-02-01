use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::logging::HiveLog;
use crate::pb::{DownloadFileChunkReq, DownloadFileChunkResp};

pub type DownloadFileChunkStream = ReceiverStream<Result<DownloadFileChunkResp, Status>>;

pub async fn handle_download_file_chunk(
    log: HiveLog,
    request: Request<DownloadFileChunkReq>,
) -> Result<Response<DownloadFileChunkStream>, Status> {
    let _g = log.enter();
    let _req = request.into_inner();
    log.warn("download_file_chunk not implemented yet");
    Err(Status::unimplemented("download_file_chunk not implemented"))
}