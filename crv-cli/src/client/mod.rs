use tonic::transport::Channel;
use crate::pb::edge_daemon_service_client::EdgeDaemonServiceClient;
use crate::pb::{GreetingReq, NilRsp};

/// gRPC 客户端结构体
pub struct CrvClient {
    client: EdgeDaemonServiceClient<Channel>,
}

impl CrvClient {
    /// 创建新的客户端实例
    pub async fn new(server_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let channel = Channel::from_shared(server_addr.to_string())?
            .connect()
            .await?;
        
        let client = EdgeDaemonServiceClient::new(channel);
        
        Ok(Self { client })
    }

    /// 发送问候消息到服务器
    pub async fn greeting(&mut self, msg: &str) -> Result<(), Box<dyn std::error::Error>> {
        let request = tonic::Request::new(GreetingReq {
            msg: msg.to_string(),
        });

        let response: tonic::Response<NilRsp> = self.client.greeting(request).await?;
        
        println!("服务器响应: {:?}", response);
        Ok(())
    }
}
