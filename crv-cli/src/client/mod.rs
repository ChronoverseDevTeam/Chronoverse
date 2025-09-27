use tonic::transport::Channel;
use crate::pb::edge_daemon_service_client::EdgeDaemonServiceClient;
use crate::pb::{BonjourReq, BonjourRsp};

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
    pub async fn bonjour(&mut self) -> Result<BonjourRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(BonjourReq {});

        let response: tonic::Response<BonjourRsp> = self.client.bonjour(request).await?;
        
        println!("服务器响应: {:?}", response);
        Ok(response.into_inner())
    }
}
