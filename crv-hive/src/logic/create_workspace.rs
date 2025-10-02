use tonic::{Request, Response, Status};

use crate::pb::{GreetingReq, NilRsp};

pub async fn greeting(
    request: Request<GreetingReq>
) -> Result<Response<NilRsp>, Status> {
    Ok(Response::new(NilRsp {}))
}