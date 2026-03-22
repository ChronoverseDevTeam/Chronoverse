use crate::hive_pb::hive_service_client::HiveServiceClient;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::Channel;
use tonic::{Request, Status};

#[derive(Clone)]
pub struct BearerInterceptor {
    token: String,
}

impl BearerInterceptor {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

impl Interceptor for BearerInterceptor {
    fn call(&mut self, mut req: Request<()>) -> Result<Request<()>, Status> {
        if !self.token.is_empty() {
            let header = format!("Bearer {}", self.token);
            let header_value = MetadataValue::try_from(header.as_str())
                .map_err(|_| Status::unauthenticated("invalid auth token"))?;
            req.metadata_mut().insert("authorization", header_value);
        }
        Ok(req)
    }
}

pub fn hive_service_client_with_bearer(
    channel: Channel,
    token: impl Into<String>,
) -> HiveServiceClient<tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>> {
    HiveServiceClient::with_interceptor(channel, BearerInterceptor::new(token))
}
