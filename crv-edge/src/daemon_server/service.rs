//! gRPC 服务 impl，仅用于路由请求到具体的 handler
use std::pin::Pin;

use super::handlers;
use super::state::AppState;
use crate::pb::file_service_server::FileService;
use crate::pb::system_service_server::SystemService;
use crate::pb::workspace_service_server::WorkspaceService;
use crate::pb::*;
use crate::{
    daemon_server::handlers::file::submit::SubmitProgressStream,
    pb::changelist_service_server::ChangelistService,
};
use tonic::{Request, Response, Status};

pub struct ChangelistServiceImpl {
    pub state: AppState,
}

impl ChangelistServiceImpl {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

type SubmitChangelistStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<SubmitProgress, Status>> + Send>>;

#[tonic::async_trait]
impl ChangelistService for ChangelistServiceImpl {
    type SubmitChangelistStream = SubmitChangelistStream;
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
    ) -> Result<Response<SubmitChangelistStream>, Status> {
        todo!()
    }
}

pub struct FileServiceImpl {
    pub state: AppState,
}

impl FileServiceImpl {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

type SyncStream = Pin<Box<dyn tokio_stream::Stream<Item = Result<SyncProgress, Status>> + Send>>;
type SubmitStream = SubmitProgressStream;

#[tonic::async_trait]
impl FileService for FileServiceImpl {
    type SyncStream = SyncStream;
    type SubmitStream = SubmitStream;

    async fn add(&self, request: Request<AddReq>) -> Result<Response<AddRsp>, Status> {
        handlers::file::add::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
    async fn checkout(&self, request: Request<CheckoutReq>) -> Result<Response<CheckoutRsp>, Status> {
        handlers::file::checkout::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
    async fn delete(&self, request: Request<DeleteReq>) -> Result<Response<DeleteRsp>, Status> {
        handlers::file::delete::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
    async fn list_active_files(&self, request: Request<ListActiveFilesReq>) -> Result<Response<ListActiveFilesRsp>, Status> {
        handlers::file::list_active_files::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
    async fn sync(&self, request: Request<SyncReq>) -> Result<Response<SyncStream>, Status> {
        handlers::file::sync::handle(self.state.clone(), request).await
            .map_err(|e| e.into())
    }
    async fn lock(&self, request: Request<LockReq>) -> Result<Response<LockRsp>, Status> {
        todo!()
    }
    async fn revert(&self, request: Request<RevertReq>) -> Result<Response<RevertRsp>, Status> {
        todo!()
    }
    async fn submit(&self, request: Request<SubmitReq>) -> Result<Response<SubmitStream>, Status> {
        handlers::file::submit::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
}

pub struct SystemServiceImpl {
    pub state: AppState,
}

impl SystemServiceImpl {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl SystemService for SystemServiceImpl {
    async fn bonjour(&self, request: Request<BonjourReq>) -> Result<Response<BonjourRsp>, Status> {
        handlers::edge::bonjour::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }

    async fn bonjour_hive(&self, request: Request<BonjourReq>) -> Result<Response<BonjourRsp>, Status> {
        handlers::edge::bonjour_hive::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }

    async fn get_runtime_config(
        &self,
        request: Request<GetRuntimeConfigReq>,
    ) -> Result<Response<GetRuntimeConfigRsp>, Status> {
        handlers::edge::get_runtime_config::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
}

pub struct WorkspaceServiceImpl {
    pub state: AppState,
}

impl WorkspaceServiceImpl {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl WorkspaceService for WorkspaceServiceImpl {
    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceReq>,
    ) -> Result<Response<CreateWorkspaceRsp>, Status> {
        handlers::workspace::create::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
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
        handlers::workspace::list::handle(self.state.clone(), request)
            .await
            .map_err(|e| e.into())
    }
    async fn describe_workspace(
        &self,
        request: Request<DescribeWorkspaceReq>,
    ) -> Result<Response<DescribeWorkspaceRsp>, Status> {
        todo!()
    }
}
