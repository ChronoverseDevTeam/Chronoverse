//! gRPC 服务 impl，仅用于路由请求到具体的 handler
use super::handlers;
use super::state::AppState;
use crate::pb::edge_daemon_service_server::EdgeDaemonService;
use crate::pb::*;
use tonic::{Request, Response, Status};

pub struct CrvEdgeDaemonServiceImpl {
    pub state: AppState,
}

impl CrvEdgeDaemonServiceImpl {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl EdgeDaemonService for CrvEdgeDaemonServiceImpl {
    async fn bonjour(&self, request: Request<BonjourReq>) -> Result<Response<BonjourRsp>, Status> {
        handlers::edge::bonjour::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceReq>,
    ) -> Result<Response<CreateWorkspaceRsp>, Status> {
        todo!()
    }
    async fn delete_workspace(
        &self,
        request: Request<DeleteWorkspaceReq>,
    ) -> Result<Response<DeleteWorkspaceRsp>, Status> {
        todo!()
    }
    async fn list_workspaces(
        &self,
        request: Request<ListWorkspacesReq>,
    ) -> Result<Response<ListWorkspacesRsp>, Status> {
        todo!()
    }
    async fn describe_workspace(
        &self,
        request: Request<DescribeWorkspaceReq>,
    ) -> Result<Response<DescribeWorkspaceRsp>, Status> {
        todo!()
    }
    async fn add(&self, request: Request<AddReq>) -> Result<Response<AddRsp>, Status> {
        todo!()
    }
    async fn sync(&self, request: Request<SyncReq>) -> Result<Response<SyncRsp>, Status> {
        todo!()
    }
    async fn lock(&self, request: Request<LockReq>) -> Result<Response<LockRsp>, Status> {
        todo!()
    }
    async fn revert(&self, request: Request<RevertReq>) -> Result<Response<RevertRsp>, Status> {
        todo!()
    }
    async fn submit(&self, request: Request<SubmitReq>) -> Result<Response<SubmitRsp>, Status> {
        todo!()
    }
    async fn create_changelist(
        &self,
        request: Request<CreateChangelistReq>,
    ) -> Result<Response<CreateChangelistRsp>, Status> {
        todo!()
    }
    async fn delete_changelist(
        &self,
        request: Request<DeleteChangelistReq>,
    ) -> Result<Response<DeleteChangelistRsp>, Status> {
        todo!()
    }
    async fn list_changelists(
        &self,
        request: Request<ListChangelistsReq>,
    ) -> Result<Response<ListChangelistsRsp>, Status> {
        todo!()
    }
    async fn describe_changelist(
        &self,
        request: Request<DescribeChangelistReq>,
    ) -> Result<Response<DescribeChangelistRsp>, Status> {
        todo!()
    }
    async fn append_changelist(
        &self,
        request: Request<AppendChangelistReq>,
    ) -> Result<Response<AppendChangelistRsp>, Status> {
        todo!()
    }
    async fn submit_changelist(
        &self,
        request: Request<SubmitChangelistReq>,
    ) -> Result<Response<SubmitChangelistRsp>, Status> {
        todo!()
    }
}
