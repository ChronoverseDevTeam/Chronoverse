use crate::auth::require_user;
use crate::common::depot_path::DepotPath;
use crate::hive_server::submit::submit_service;
use crate::pb::{FileRevision as PbFileRevision, SubmitConflict as PbSubmitConflict, SubmitReq, SubmitRsp, UploadFileChunkRsp};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub type UploadFileChunkStream = ReceiverStream<Result<UploadFileChunkRsp, Status>>;

pub async fn submit(
    r: Request<SubmitReq>,
) -> Result<Response<SubmitRsp>, Status> {
    let user = require_user(&r)?;
    let submitting_by = user.username.clone();
    let request = r.into_inner();

    let service = submit_service();

    let ticket_uuid = uuid::Uuid::parse_str(&request.ticket)
        .map_err(|e| Status::invalid_argument(format!("invalid ticket format: {e}")))?;

    let mut validations: std::collections::HashMap<DepotPath, Vec<String>> =
        std::collections::HashMap::new();

    for fc in &request.file_chunks {
        let path = DepotPath::new(&fc.path)
            .map_err(|e| Status::invalid_argument(format!("invalid depot path '{}': {e}", fc.path)))?;
        validations.insert(path, fc.binary_id.clone());
    }

    let result = service
        .submit(&ticket_uuid, request.description.clone(), validations)
        .await;

    let rsp = match result {
        Ok(success) => SubmitRsp {
            success: true,
            changelist_id: success.changelist_id,
            committed_at: success.committed_at,
            conflicts: vec![],
            missing_chunks: vec![],
            latest_revisions: success
                .latest_revisions
                .into_iter()
                .map(|r| PbFileRevision {
                    path: r.path,
                    generation: r.generation,
                    revision: r.revision,
                    binary_id: r.binary_id,
                    size: r.size,
                    revision_created_at: r.revision_created_at,
                })
                .collect(),
            message: format!("submitted by {}", submitting_by),
        },
        Err(failure) => SubmitRsp {
            success: false,
            changelist_id: 0,
            committed_at: 0,
            conflicts: failure
                .conflicts
                .into_iter()
                .map(|c| PbSubmitConflict {
                    path: c.path,
                    expected_file_generation: c.expected_generation,
                    expected_file_revision: c.expected_revision,
                    current_file_generation: c.current_generation,
                    current_file_revision: c.current_revision,
                })
                .collect(),
            missing_chunks: failure.missing_chunks,
            latest_revisions: vec![],
            message: failure.message,
        },
    };
    Ok(Response::new(rsp))
}