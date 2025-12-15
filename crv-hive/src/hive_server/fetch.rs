use crate::auth;
use crate::hive_server::{depot_tree, hive_dao as dao};
use crate::pb::{
    file_tree_node,
    Directory as PbDirectory,
    File as PbFile,
    FileTreeNode as PbFileTreeNode,
    GetFileTreeReq,
    GetFileTreeRsp,
};
use crv_core::metadata::{BranchDoc, ChangelistDoc, FileDoc, FileRevisionDoc};
use crv_core::tree::{FileTree, FileTreeError, FileTreeNode};
use tonic::{Request, Response, Status};

fn file_tree_error_to_status(e: FileTreeError) -> Status {
    match e {
        FileTreeError::BranchNotFound(msg) => Status::not_found(msg),
        FileTreeError::ChangelistNotFound(id) => Status::not_found(format!("changelist not found: {id}")),
        FileTreeError::BranchMismatch {
            branch_id,
            changelist_id,
            actual_branch_id,
        } => Status::failed_precondition(format!(
            "changelist {changelist_id} belongs to branch {actual_branch_id}, expected {branch_id}"
        )),
        FileTreeError::InvalidDepotPathWildcard(msg) => Status::invalid_argument(msg),
        FileTreeError::Backend(msg) => Status::internal(msg),
    }
}

fn core_node_to_pb(node: &FileTreeNode) -> PbFileTreeNode {
    match node {
        FileTreeNode::Directory { name, children } => PbFileTreeNode {
            node: Some(file_tree_node::Node::Directory(PbDirectory {
                name: name.clone(),
                children: children.iter().map(core_node_to_pb).collect(),
            })),
        },
        FileTreeNode::File {
            name,
            file_id,
            reivision_id,
            changelist_id,
            binary_id,
            size,
            revision_created_at,
        } => PbFileTreeNode {
            node: Some(file_tree_node::Node::File(PbFile {
                name: name.clone(),
                file_id: file_id.clone(),
                revision_id: reivision_id.clone(),
                changelist_id: changelist_id.to_string(),
                binary_id: binary_id.clone(),
                size: *size,
                revision_created_at: *revision_created_at,
            })),
        },
    }
}

pub async fn handle_get_file_tree(
    request: Request<GetFileTreeReq>,
) -> Result<Response<GetFileTreeRsp>, Status> {
    // 所有调用必须已通过鉴权拦截器注入 UserContext；否则直接拒绝。
    let _user = auth::require_user(&request)?;

    let req = request.into_inner();
    let branch_id = req.branch_id.trim().to_string();
    if branch_id.is_empty() {
        return Err(Status::invalid_argument("branch_id is required"));
    }

    let depot_wildcard = req.depot_wildcard.trim().to_string();
    if depot_wildcard.is_empty() {
        return Err(Status::invalid_argument("depot_wildcard is required"));
    }

    // 兼容：若未指定 changelist_id（<=0），默认读取分支 HEAD。
    let mut changelist_id = req.changelist_id;
    if changelist_id <= 0 {
        let branch = dao::find_branch_by_id(&branch_id)
            .await
            .map_err(|e| Status::internal(format!("database error while reading branch: {e}")))?
            .ok_or_else(|| Status::not_found(format!("branch not found: {branch_id}")))?;
        changelist_id = branch.head_changelist_id;
    }

    // 1) 先尝试走内存缓存
    let cached: Option<FileTree> = {
        let guard = depot_tree().lock().await;
        guard
            .get_cached_file_tree(&branch_id, changelist_id, &depot_wildcard)
            .cloned()
    };

    let tree = if let Some(t) = cached {
        t
    } else {
        // 2) 缓存未命中：构建树（该过程会回溯 changelist 链，可能较重，放到 blocking 线程池）
        let rt = tokio::runtime::Handle::current();
        let branch_id_cloned = branch_id.clone();
        let depot_wildcard_cloned = depot_wildcard.clone();

        let tree = tokio::task::spawn_blocking(move || {
            let rt = rt.clone();

            let mut get_branch = |id: &str| -> Result<Option<BranchDoc>, String> {
                rt.block_on(async {
                    dao::find_branch_by_id(id)
                        .await
                        .map_err(|e| e.to_string())
                })
            };

            let mut get_changelist = |id: i64| -> Result<Option<ChangelistDoc>, String> {
                rt.block_on(async {
                    dao::find_changelist_by_id(id)
                        .await
                        .map_err(|e| e.to_string())
                })
            };

            let mut get_file = |id: &str| -> Result<Option<FileDoc>, String> {
                rt.block_on(async {
                    dao::find_file_by_id(id)
                        .await
                        .map_err(|e| e.to_string())
                })
            };

            let mut get_file_revision = |id: &str| -> Result<Option<FileRevisionDoc>, String> {
                rt.block_on(async {
                    dao::find_file_revision_by_id(id)
                        .await
                        .map_err(|e| e.to_string())
                })
            };

            crv_core::tree::construct_tree_from_changelist(
                &branch_id_cloned,
                &depot_wildcard_cloned,
                changelist_id,
                &mut get_branch,
                &mut get_changelist,
                &mut get_file,
                &mut get_file_revision,
            )
        })
        .await
        .map_err(|e| Status::internal(format!("failed to join file tree task: {e}")))?
        .map_err(file_tree_error_to_status)?;

        // 写入缓存
        {
            let mut guard = depot_tree().lock().await;
            guard.cache_file_tree(&branch_id, changelist_id, &depot_wildcard, tree.clone());
        }

        tree
    };

    let rsp = GetFileTreeRsp {
        file_tree_root: tree.nodes.iter().map(core_node_to_pb).collect(),
    };
    Ok(Response::new(rsp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthService, AuthSource, TokenPolicy, UserContext};
    use crate::hive_server::{CrvHiveService, hive_dao};
    use crate::pb::hive_service_server::HiveService;
    use crv_core::metadata::{
        BranchDoc, BranchMetadata, ChangelistAction, ChangelistChange, ChangelistDoc,
        ChangelistMetadata, FileDoc, FileMetadata, FileRevisionDoc, FileRevisionMetadata,
    };
    use std::sync::Arc;
    use tonic::{Code, Request};

    fn make_test_auth() -> Arc<AuthService> {
        Arc::new(AuthService::new(
            b"test-secret",
            TokenPolicy {
                ttl_secs: 60,
                renew_before_secs: 30,
            },
        ))
    }

    fn make_service() -> CrvHiveService {
        CrvHiveService::new(make_test_auth())
    }

    fn make_authed_request<T>(msg: T) -> Request<T> {
        let mut req = Request::new(msg);
        req.extensions_mut().insert(UserContext {
            username: "test-user".to_string(),
            scopes: Vec::new(),
            source: AuthSource::Jwt,
        });
        req
    }

    async fn reset_isolated_state(branch_id: &str) {
        // Mock DAO 是进程级全局状态，测试前统一清空，避免并发污染。
        hive_dao::reset_all();

        // DepotTree 也是进程级全局状态，清掉该分支下的 file tree cache，避免跨测试污染。
        let mut tree = crate::hive_server::depot_tree().lock().await;
        tree.clear_all_file_tree_cache(branch_id);
    }

    async fn seed_minimal_tree_data(
        branch_id: &str,
        head_changelist_id: i64,
        file_id: &str,
        file_path: &str,
        revision_id: &str,
    ) {
        let branch = BranchDoc {
            id: branch_id.to_string(),
            created_at: 0,
            created_by: "tester".to_string(),
            head_changelist_id,
            metadata: BranchMetadata {
                description: "test branch".to_string(),
                owners: vec!["tester".to_string()],
            },
        };
        hive_dao::put_branch(branch);

        let cl = ChangelistDoc {
            id: head_changelist_id,
            parent_changelist_id: 0,
            branch_id: branch_id.to_string(),
            author: "tester".to_string(),
            description: "seed".to_string(),
            changes: vec![ChangelistChange {
                file: file_id.to_string(),
                action: ChangelistAction::Create,
                revision: revision_id.to_string(),
            }],
            committed_at: 0,
            files_count: 1,
            metadata: ChangelistMetadata { labels: vec![] },
        };
        hive_dao::insert_changelist(cl)
            .await
            .expect("insert_changelist should succeed");

        let file = FileDoc {
            id: file_id.to_string(),
            path: file_path.to_string(),
            created_at: 0,
            metadata: FileMetadata {
                first_introduced_by: "tester".to_string(),
            },
        };
        hive_dao::insert_file(file)
            .await
            .expect("insert_file should succeed");

        let rev = FileRevisionDoc {
            id: revision_id.to_string(),
            branch_id: branch_id.to_string(),
            file_id: file_id.to_string(),
            changelist_id: head_changelist_id,
            binary_id: vec!["deadbeef".to_string()],
            parent_revision_id: String::new(),
            size: 123,
            is_delete: false,
            created_at: 456,
            metadata: FileRevisionMetadata {
                file_mode: "755".to_string(),
                hash: "deadbeef".to_string(),
                is_binary: true,
                language: String::new(),
            },
        };
        hive_dao::insert_file_revisions(vec![rev])
            .await
            .expect("insert_file_revisions should succeed");
    }

    #[tokio::test]
    async fn get_file_tree_requires_auth() {
        let _guard = crate::hive_server::test_global_lock().await;
        let service = make_service();
        let req = GetFileTreeReq {
            branch_id: "fetch_auth_branch".to_string(),
            depot_wildcard: "//src/...".to_string(),
            changelist_id: 1,
        };

        let res = service.get_file_tree(Request::new(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn get_file_tree_requires_branch_id() {
        let _guard = crate::hive_server::test_global_lock().await;
        let req = GetFileTreeReq {
            branch_id: "   ".to_string(),
            depot_wildcard: "//src/...".to_string(),
            changelist_id: 1,
        };

        let res = handle_get_file_tree(make_authed_request(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::InvalidArgument);
        assert_eq!(status.message(), "branch_id is required");
    }

    #[tokio::test]
    async fn get_file_tree_requires_depot_wildcard() {
        let _guard = crate::hive_server::test_global_lock().await;
        let req = GetFileTreeReq {
            branch_id: "fetch_wildcard_required_branch".to_string(),
            depot_wildcard: "   ".to_string(),
            changelist_id: 1,
        };

        let res = handle_get_file_tree(make_authed_request(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::InvalidArgument);
        assert_eq!(status.message(), "depot_wildcard is required");
    }

    #[tokio::test]
    async fn get_file_tree_defaults_to_branch_head_when_changelist_id_non_positive() {
        let _guard = crate::hive_server::test_global_lock().await;
        let branch_id = "fetch_default_head_branch";
        reset_isolated_state(branch_id).await;

        seed_minimal_tree_data(
            branch_id,
            100,
            "file_1",
            "//src/main.cpp",
            "rev_1",
        )
        .await;

        // changelist_id <= 0 时应回退到 branch.head_changelist_id
        let req = GetFileTreeReq {
            branch_id: branch_id.to_string(),
            depot_wildcard: "//src/...".to_string(),
            changelist_id: 0,
        };

        let rsp = handle_get_file_tree(make_authed_request(req))
            .await
            .expect("get_file_tree should succeed")
            .into_inner();

        assert_eq!(rsp.file_tree_root.len(), 1);
        let node = rsp.file_tree_root[0].node.as_ref().expect("node should exist");
        match node {
            file_tree_node::Node::File(f) => {
                assert_eq!(f.name, "main.cpp");
                assert_eq!(f.file_id, "file_1");
                assert_eq!(f.revision_id, "rev_1");
                assert_eq!(f.changelist_id, "100");
                assert_eq!(f.size, 123);
                assert_eq!(f.revision_created_at, 456);
            }
            file_tree_node::Node::Directory(_) => panic!("expected File node at //src root"),
        }
    }

    #[tokio::test]
    async fn get_file_tree_uses_cache_when_called_twice() {
        let _guard = crate::hive_server::test_global_lock().await;
        let branch_id = "fetch_cache_branch";
        reset_isolated_state(branch_id).await;

        seed_minimal_tree_data(
            branch_id,
            200,
            "file_cache_1",
            "//src/a.txt",
            "rev_cache_1",
        )
        .await;

        let req = GetFileTreeReq {
            branch_id: branch_id.to_string(),
            depot_wildcard: "//src/...".to_string(),
            changelist_id: 200,
        };

        let rsp1 = handle_get_file_tree(make_authed_request(req.clone()))
            .await
            .expect("first get_file_tree should succeed")
            .into_inner();
        assert_eq!(rsp1.file_tree_root.len(), 1);

        // 清空 DAO：若第二次还成功，说明是命中 DepotTree 缓存而不是重新走 DAO 构建。
        hive_dao::reset_all();

        let rsp2 = handle_get_file_tree(make_authed_request(req))
            .await
            .expect("second get_file_tree should succeed due to cache hit")
            .into_inner();
        assert_eq!(rsp2.file_tree_root.len(), 1);
    }

    #[tokio::test]
    async fn get_file_tree_returns_not_found_when_changelist_missing() {
        let _guard = crate::hive_server::test_global_lock().await;
        let branch_id = "fetch_missing_changelist_branch";
        reset_isolated_state(branch_id).await;

        // 仅插入分支，不插入 changelist。
        let branch = BranchDoc {
            id: branch_id.to_string(),
            created_at: 0,
            created_by: "tester".to_string(),
            head_changelist_id: 1,
            metadata: BranchMetadata {
                description: "test".to_string(),
                owners: vec!["tester".to_string()],
            },
        };
        hive_dao::put_branch(branch);

        let req = GetFileTreeReq {
            branch_id: branch_id.to_string(),
            depot_wildcard: "//src/...".to_string(),
            changelist_id: 999,
        };

        let res = handle_get_file_tree(make_authed_request(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::NotFound);
        assert_eq!(status.message(), "changelist not found: 999");
    }

    #[tokio::test]
    async fn get_file_tree_maps_invalid_wildcard_to_invalid_argument() {
        let _guard = crate::hive_server::test_global_lock().await;
        let branch_id = "fetch_invalid_wildcard_branch";
        reset_isolated_state(branch_id).await;

        // 需要保证 branch/changelist 存在，才能走到通配符解析逻辑。
        let branch = BranchDoc {
            id: branch_id.to_string(),
            created_at: 0,
            created_by: "tester".to_string(),
            head_changelist_id: 1,
            metadata: BranchMetadata {
                description: "test".to_string(),
                owners: vec!["tester".to_string()],
            },
        };
        hive_dao::put_branch(branch);

        let cl = ChangelistDoc {
            id: 1,
            parent_changelist_id: 0,
            branch_id: branch_id.to_string(),
            author: "tester".to_string(),
            description: "seed".to_string(),
            changes: vec![],
            committed_at: 0,
            files_count: 0,
            metadata: ChangelistMetadata { labels: vec![] },
        };
        hive_dao::insert_changelist(cl)
            .await
            .expect("insert_changelist should succeed");

        let req = GetFileTreeReq {
            branch_id: branch_id.to_string(),
            // 明显非法的 depot wildcard
            depot_wildcard: "not-a-depot-wildcard".to_string(),
            changelist_id: 1,
        };

        let res = handle_get_file_tree(make_authed_request(req)).await;
        assert!(res.is_err());
        let status = res.err().unwrap();
        assert_eq!(status.code(), Code::InvalidArgument);
    }
}

