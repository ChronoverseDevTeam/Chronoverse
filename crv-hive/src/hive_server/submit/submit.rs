use crate::pb::UploadFileChunkRsp;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

pub type UploadFileChunkStream = ReceiverStream<Result<UploadFileChunkRsp, Status>>;

