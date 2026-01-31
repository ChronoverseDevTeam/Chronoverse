use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use crate::pb::{DownloadFileChunkReq, DownloadFileChunkResp};

pub type DownloadFileChunkStream = ReceiverStream<Result<DownloadFileChunkResp, Status>>;

pub async fn handle_download_file_chunk(
    request: Request<DownloadFileChunkReq>,
) -> Result<Response<DownloadFileChunkStream>, Status> {
    let req = request.into_inner();
    panic!("todo")
}