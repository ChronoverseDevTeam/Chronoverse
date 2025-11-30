use tonic::{Request, Response, Status};

use crate::pb::{FileChunk, NilRsp};

pub async fn upload(request: Request<tonic::Streaming<FileChunk>>) -> Result<Response<NilRsp>, Status> {
    let request = request.into_inner();
    Ok(Response::new(NilRsp {}))
}