use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::metadata::{BranchDoc, ChangelistAction, ChangelistDoc, FileDoc, FileRevisionDoc};
use crate::path::basic::{DepotPath, DepotPathWildcard};
use thiserror::Error;

pub mod depot_tree;

/// 文件树整体结构，描述某个根目录下的层级关系
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTree {
    /// 根目录下的所有顶层节点
    pub nodes: Vec<FileTreeNode>,
}

/// 文件树中的节点，包含目录与文件两种类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "nodeType", rename_all = "camelCase")]
pub enum FileTreeNode {
    /// 目录节点
    Directory {
        /// 目录名（不包含上级路径）
        name: String,
        /// 子节点列表
        children: Vec<FileTreeNode>,
    },
    /// 文件节点
    File {
        /// 文件名（不包含上级路径）
        name: String,
        /// 文件的唯一 ID
        file_id: String,
        /// 文件当前版本的 revision_id
        reivision_id: String,
        /// 文件当前版本属于的 Changelist
        changelist_id: i64,
        /// 当前版本的二进制数据索引
        binary_id: Vec<String>,
        /// 文件大小（字节）
        size: i64,
        /// 当前文件 revision 的创建时间
        revision_created_at: i64,
    },
}

/// 构建文件树时可能出现的错误
#[derive(Debug, Error)]
pub enum FileTreeError {
    #[error("分支不存在: {0}")]
    BranchNotFound(String),
    #[error("Changelist 不存在: {0}")]
    ChangelistNotFound(i64),
    #[error(
        "Changelist {changelist_id} 的分支为 {actual_branch_id}，与期望的分支 {branch_id} 不一致"
    )]
    BranchMismatch {
        branch_id: String,
        changelist_id: i64,
        actual_branch_id: String,
    },
    #[error("无效的 depot 路径通配符: {0}")]
    InvalidDepotPathWildcard(String),
    #[error("底层存储错误: {0}")]
    Backend(String),
}

pub type FileTreeResult<T> = Result<T, FileTreeError>;

/// 从指定分支、指定 changelist 的文件快照，构建给定路径下的目录树。
///
/// - `branch_id`：目标分支 ID。
/// - `changelist_id`：目标 changelist ID。
/// - `depot_wildcard`：类似 `//src/module/...` 的 depot 路径通配符，仅支持范围通配形式。
/// - `get_*` 系列函数：由调用方提供的访问后端存储的函数，用于按 ID 读取对象。
#[allow(non_snake_case)]
pub fn construct_tree_from_changelist<GB, GC, GF, GR>(
    branch_id: &str,
    depot_wildcard: &str,
    changelist_id: i64,
    mut get_branch: GB,
    mut get_changelist: GC,
    mut get_file: GF,
    mut get_file_revision: GR,
) -> FileTreeResult<FileTree>
where
    GB: FnMut(&str) -> Result<Option<BranchDoc>, String>,
    GC: FnMut(i64) -> Result<Option<ChangelistDoc>, String>,
    GF: FnMut(&str) -> Result<Option<FileDoc>, String>,
    GR: FnMut(&str) -> Result<Option<FileRevisionDoc>, String>,
{
    // 1. 校验分支和 changelist 基本信息
    let _branch = get_branch(branch_id)
        .map_err(FileTreeError::Backend)?
        .ok_or_else(|| FileTreeError::BranchNotFound(branch_id.to_string()))?;

    let changelist = get_changelist(changelist_id)
        .map_err(FileTreeError::Backend)?
        .ok_or(FileTreeError::ChangelistNotFound(changelist_id))?;

    if changelist.branch_id != branch_id {
        return Err(FileTreeError::BranchMismatch {
            branch_id: branch_id.to_string(),
            changelist_id,
            actual_branch_id: changelist.branch_id,
        });
    }

    // 2. 解析 depot 路径通配符，只支持范围通配（Range）形式
    let wildcard = DepotPathWildcard::parse(depot_wildcard)
        .map_err(|e| FileTreeError::InvalidDepotPathWildcard(e.to_string()))?;

    let range_wildcard = match wildcard {
        DepotPathWildcard::Range(r) => r,
        DepotPathWildcard::Regex(_) => {
            return Err(FileTreeError::InvalidDepotPathWildcard(
                "构建文件树暂不支持正则通配符，请使用 //path/... 形式的范围通配".to_string(),
            ));
        }
    };

    // 3. 自顶向下回溯 changelist 链，计算在目标 changelist 下可见的文件最新 revision
    //
    // key: file_id
    // value: Some(revision_id) -> 该文件在目标 changelist 下的可见版本
    //        None              -> 在目标 changelist 下已被删除
    let mut visible: HashMap<String, Option<String>> = HashMap::new();

    let mut current_id = changelist_id;
    // 避免极端情况下的死循环，这里简单做一个最大步数保护
    let mut steps: u32 = 0;

    while current_id > 0 {
        let cl = match get_changelist(current_id)
            .map_err(FileTreeError::Backend)?
        {
            Some(c) => c,
            None => break, // 提前结束：历史链中断
        };

        // 额外安全校验：如果遇到不同分支的 changelist，则终止（防御性写法）
        if cl.branch_id != branch_id {
            break;
        }

        for change in &cl.changes {
            // 如果该文件已经在更靠近 HEAD 的 changelist 中被处理过，就跳过
            if visible.contains_key(&change.file) {
                continue;
            }

            match change.action {
                ChangelistAction::Delete => {
                    visible.insert(change.file.clone(), None);
                }
                ChangelistAction::Create | ChangelistAction::Modify => {
                    visible.insert(change.file.clone(), Some(change.revision.clone()));
                }
            }
        }

        if cl.parent_changelist_id <= 0 {
            break;
        }
        current_id = cl.parent_changelist_id;

        steps += 1;
        if steps > 1_000_000 {
            // 极端情况下的保护，防止错误数据导致的无限循环
            break;
        }
    }

    // 4. 使用中间结构按目录层级组织文件，然后再转换为 FileTree
    #[derive(Default)]
    struct DirNode {
        children: BTreeMap<String, DirNode>,
        files: BTreeMap<String, FileTreeNode>,
    }

    fn insert_file(
        root: &mut DirNode,
        dir_parts: &[String],
        file_node: FileTreeNode,
    ) {
        let mut current = root;
        for part in dir_parts {
            current = current
                .children
                .entry(part.clone())
                .or_insert_with(DirNode::default);
        }
        if let FileTreeNode::File { name, .. } = &file_node {
            current.files.insert(name.clone(), file_node);
        }
    }

    fn to_nodes(node: DirNode) -> Vec<FileTreeNode> {
        let mut result = Vec::new();

        // 目录（按名称排序）
        for (name, child) in node.children {
            let children = to_nodes(child);
            result.push(FileTreeNode::Directory { name, children });
        }

        // 文件（按名称排序）
        for (_name, file_node) in node.files {
            result.push(file_node);
        }

        result
    }

    let mut root = DirNode::default();

    // 5. 遍历可见文件，过滤路径并插入树中
    for (file_id, revision_opt) in visible {
        let revision_id = match revision_opt {
            Some(r) => r,
            None => continue, // 在目标 changelist 下已被删除
        };

        let revision = match get_file_revision(&revision_id)
            .map_err(FileTreeError::Backend)?
        {
            Some(r) => r,
            None => {
                return Err(FileTreeError::Backend(format!(
                    "找不到 revision，id={revision_id}"
                )));
            }
        };

        // 只保留目标分支的 revision，防御性校验
        if revision.branch_id != branch_id {
            continue;
        }

        let file = match get_file(&file_id)
            .map_err(FileTreeError::Backend)?
        {
            Some(f) => f,
            None => {
                return Err(FileTreeError::Backend(format!(
                    "找不到文件，id={file_id}"
                )));
            }
        };

        // 将 FileDoc 的路径解析为 DepotPath
        let depot_path =
            DepotPath::parse(&file.path).map_err(|e| FileTreeError::Backend(e.to_string()))?;

        // 使用范围通配符过滤路径，并获取相对于基准路径的目录差分
        let diff = match range_wildcard.match_and_get_diff(&depot_path) {
            Some(d) => d,
            None => continue, // 不在指定路径下，跳过
        };

        // diff 即为相对于基准路径（例如 //src/module）下的子目录列表
        let relative_dirs: Vec<String> = diff.to_vec();

        let file_node = FileTreeNode::File {
            name: depot_path.file,
            file_id: file.id,
            reivision_id: revision.id,
            changelist_id: revision.changelist_id,
            binary_id: revision.binary_id.clone(),
            size: revision.size,
            revision_created_at: revision.created_at,
        };

        insert_file(&mut root, &relative_dirs, file_node);
    }

    Ok(FileTree {
        nodes: to_nodes(root),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{
        BranchMetadata, ChangelistChange, ChangelistMetadata, FileMetadata, FileRevisionMetadata,
    };
    use std::collections::HashMap;

    fn collect_files(nodes: &[FileTreeNode], out: &mut HashMap<String, String>) {
        for node in nodes {
            match node {
                FileTreeNode::Directory { children, .. } => {
                    collect_files(children, out);
                }
                FileTreeNode::File {
                    file_id,
                    reivision_id,
                    ..
                } => {
                    out.insert(file_id.clone(), reivision_id.clone());
                }
            }
        }
    }

    fn build_common_branch() -> BranchDoc {
        BranchDoc {
            id: "branch_main".to_string(),
            created_at: 0,
            created_by: "userA".to_string(),
            head_changelist_id: 300,
            metadata: BranchMetadata {
                description: "main".to_string(),
            },
        }
    }

    fn build_file_docs() -> HashMap<String, FileDoc> {
        let mut files = HashMap::new();
        files.insert(
            "f1".to_string(),
            FileDoc {
                id: "f1".to_string(),
                path: "//src/module/a.cpp".to_string(),
                seen_on_branches: vec!["branch_main".to_string()],
                created_at: 0,
                metadata: FileMetadata {
                    first_introduced_by: "userA".to_string(),
                },
            },
        );
        files.insert(
            "f2".to_string(),
            FileDoc {
                id: "f2".to_string(),
                path: "//src/other/b.cpp".to_string(),
                seen_on_branches: vec!["branch_main".to_string()],
                created_at: 0,
                metadata: FileMetadata {
                    first_introduced_by: "userA".to_string(),
                },
            },
        );
        files
    }

    fn build_file_revisions() -> HashMap<String, FileRevisionDoc> {
        let mut revs = HashMap::new();
        // f1 在 CL 100 创建
        revs.insert(
            "r1".to_string(),
            FileRevisionDoc {
                id: "r1".to_string(),
                branch_id: "branch_main".to_string(),
                file_id: "f1".to_string(),
                changelist_id: 100,
                binary_id: vec!["blob_r1".to_string()],
                parent_revision_id: "".to_string(),
                size: 10,
                is_delete: false,
                created_at: 1,
                metadata: FileRevisionMetadata {
                    file_mode: "755".to_string(),
                    hash: "h1".to_string(),
                    is_binary: false,
                    language: "cpp".to_string(),
                },
            },
        );
        // f1 在 CL 200 修改
        revs.insert(
            "r2".to_string(),
            FileRevisionDoc {
                id: "r2".to_string(),
                branch_id: "branch_main".to_string(),
                file_id: "f1".to_string(),
                changelist_id: 200,
                binary_id: vec!["blob_r2".to_string()],
                parent_revision_id: "r1".to_string(),
                size: 20,
                is_delete: false,
                created_at: 2,
                metadata: FileRevisionMetadata {
                    file_mode: "755".to_string(),
                    hash: "h2".to_string(),
                    is_binary: false,
                    language: "cpp".to_string(),
                },
            },
        );
        // f2 在 CL 200 创建（不会匹配 //src/module/...，仅用于完整性）
        revs.insert(
            "r3_unused".to_string(),
            FileRevisionDoc {
                id: "r3_unused".to_string(),
                branch_id: "branch_main".to_string(),
                file_id: "f2".to_string(),
                changelist_id: 200,
                binary_id: vec!["blob_r3".to_string()],
                parent_revision_id: "".to_string(),
                size: 5,
                is_delete: false,
                created_at: 3,
                metadata: FileRevisionMetadata {
                    file_mode: "755".to_string(),
                    hash: "h3".to_string(),
                    is_binary: false,
                    language: "cpp".to_string(),
                },
            },
        );
        revs
    }

    fn build_changelists() -> HashMap<i64, ChangelistDoc> {
        let mut cls = HashMap::new();
        // CL 100: 创建 f1
        cls.insert(
            100,
            ChangelistDoc {
                id: 100,
                parent_changelist_id: 0,
                branch_id: "branch_main".to_string(),
                author: "userA".to_string(),
                description: "create f1".to_string(),
                changes: vec![ChangelistChange {
                    file: "f1".to_string(),
                    action: ChangelistAction::Create,
                    revision: "r1".to_string(),
                }],
                committed_at: 1,
                files_count: 1,
                metadata: ChangelistMetadata { labels: vec![] },
            },
        );
        // CL 200: 修改 f1，并创建 f2（不在 //src/module/... 下）
        cls.insert(
            200,
            ChangelistDoc {
                id: 200,
                parent_changelist_id: 100,
                branch_id: "branch_main".to_string(),
                author: "userA".to_string(),
                description: "modify f1; create f2".to_string(),
                changes: vec![
                    ChangelistChange {
                        file: "f1".to_string(),
                        action: ChangelistAction::Modify,
                        revision: "r2".to_string(),
                    },
                    ChangelistChange {
                        file: "f2".to_string(),
                        action: ChangelistAction::Create,
                        revision: "r3_unused".to_string(),
                    },
                ],
                committed_at: 2,
                files_count: 2,
                metadata: ChangelistMetadata { labels: vec![] },
            },
        );
        // CL 300: 删除 f1
        cls.insert(
            300,
            ChangelistDoc {
                id: 300,
                parent_changelist_id: 200,
                branch_id: "branch_main".to_string(),
                author: "userA".to_string(),
                description: "delete f1".to_string(),
                changes: vec![ChangelistChange {
                    file: "f1".to_string(),
                    action: ChangelistAction::Delete,
                    revision: "r2".to_string(),
                }],
                committed_at: 3,
                files_count: 1,
                metadata: ChangelistMetadata { labels: vec![] },
            },
        );
        cls
    }

    #[test]
    fn construct_tree_basic_visible_file() {
        let branch = build_common_branch();
        let files = build_file_docs();
        let revs = build_file_revisions();
        let cls = build_changelists();

        let get_branch = move |id: &str| {
            if id == branch.id {
                Ok(Some(branch.clone()))
            } else {
                Ok(None)
            }
        };

        let get_changelist = move |id: i64| Ok(cls.get(&id).cloned());
        let get_file = move |id: &str| Ok(files.get(id).cloned());
        let get_file_revision = move |id: &str| Ok(revs.get(id).cloned());

        let tree = construct_tree_from_changelist(
            "branch_main",
            "//src/module/...",
            200,
            get_branch,
            get_changelist,
            get_file,
            get_file_revision,
        )
        .expect("constructTreeFromHead should succeed");

        // 期待在 //src/module/... 下只有一个文件 a.cpp，且来自 revision r2
        assert_eq!(tree.nodes.len(), 1);
        match &tree.nodes[0] {
            FileTreeNode::File {
                name,
                file_id,
                reivision_id,
                ..
            } => {
                assert_eq!(name, "a.cpp");
                assert_eq!(file_id, "f1");
                assert_eq!(reivision_id, "r2");
            }
            other => panic!("unexpected node: {:?}", other),
        }
    }

    #[test]
    fn construct_tree_respects_delete() {
        let branch = build_common_branch();
        let files = build_file_docs();
        let revs = build_file_revisions();
        let cls = build_changelists();

        let get_branch = move |id: &str| {
            if id == branch.id {
                Ok(Some(branch.clone()))
            } else {
                Ok(None)
            }
        };

        let get_changelist = move |id: i64| Ok(cls.get(&id).cloned());
        let get_file = move |id: &str| Ok(files.get(id).cloned());
        let get_file_revision = move |id: &str| Ok(revs.get(id).cloned());

        // 在 CL 300 之后，f1 被删除，因此树应为空
        let tree = construct_tree_from_changelist(
            "branch_main",
            "//src/module/...",
            300,
            get_branch,
            get_changelist,
            get_file,
            get_file_revision,
        )
        .expect("constructTreeFromHead should succeed");

        assert!(tree.nodes.is_empty());
    }

    #[test]
    fn construct_tree_long_changelist_chain() {
        // 构造一个 10 层 changelist 链，包含多个文件，且每个 changelist 上对文件进行“伪随机”的
        // 创建 / 修改 / 删除操作，用于验证长链条下可见性计算是否正确。
        use crate::metadata::ChangelistDoc;

        let branch = BranchDoc {
            id: "branch_rand".to_string(),
            created_at: 0,
            created_by: "userLong".to_string(),
            head_changelist_id: 10,
            metadata: BranchMetadata {
                description: "long random branch".to_string(),
            },
        };

        // 三个文件，其中两个在 //src/module/... 下，一个在其他路径
        let mut files: HashMap<String, FileDoc> = HashMap::new();
        files.insert(
            "fa".to_string(),
            FileDoc {
                id: "fa".to_string(),
                path: "//src/module/fa.txt".to_string(),
                seen_on_branches: vec!["branch_rand".to_string()],
                created_at: 0,
                metadata: FileMetadata {
                    first_introduced_by: "userLong".to_string(),
                },
            },
        );
        files.insert(
            "fb".to_string(),
            FileDoc {
                id: "fb".to_string(),
                path: "//src/module/deep/fb.txt".to_string(),
                seen_on_branches: vec!["branch_rand".to_string()],
                created_at: 0,
                metadata: FileMetadata {
                    first_introduced_by: "userLong".to_string(),
                },
            },
        );
        files.insert(
            "fc".to_string(),
            FileDoc {
                id: "fc".to_string(),
                path: "//src/other/fc.txt".to_string(),
                seen_on_branches: vec!["branch_rand".to_string()],
                created_at: 0,
                metadata: FileMetadata {
                    first_introduced_by: "userLong".to_string(),
                },
            },
        );

        let file_ids = vec!["fa".to_string(), "fb".to_string(), "fc".to_string()];

        // 每个文件当前是否存在
        let mut exists: HashMap<String, bool> =
            file_ids.iter().map(|id| (id.clone(), false)).collect();
        // 每个文件的 revision 计数器
        let mut rev_counters: HashMap<String, i32> =
            file_ids.iter().map(|id| (id.clone(), 0)).collect();

        let mut revs: HashMap<String, FileRevisionDoc> = HashMap::new();
        let mut cls: HashMap<i64, ChangelistDoc> = HashMap::new();
        // 记录期望的可见状态（与 constructTreeFromHead 内部逻辑一致）
        let mut expected_visible: HashMap<String, Option<String>> = HashMap::new();

        for i in 1_i64..=10 {
            let parent_id = if i == 1 { 0 } else { i - 1 };

            // 伪随机选择一个文件：用 i * 7 mod 3
            let idx = ((i * 7) as usize) % file_ids.len();
            let file_id = file_ids[idx].clone();

            let currently_exists = *exists.get(&file_id).unwrap();

            let (action, revision_id_opt) = if !currently_exists {
                // 文件当前不存在，则只能创建
                let counter = rev_counters.entry(file_id.clone()).or_insert(0);
                *counter += 1;
                let rev_id = format!("{}_r{}", file_id, counter);
                revs.insert(
                    rev_id.clone(),
                    FileRevisionDoc {
                        id: rev_id.clone(),
                        branch_id: "branch_rand".to_string(),
                        file_id: file_id.clone(),
                        changelist_id: i,
                        binary_id: vec![format!("blob_{rev_id}")],
                        parent_revision_id: String::new(),
                        size: 10 * i,
                        is_delete: false,
                        created_at: i,
                        metadata: FileRevisionMetadata {
                            file_mode: "755".to_string(),
                            hash: format!("h_{rev_id}"),
                            is_binary: false,
                            language: "txt".to_string(),
                        },
                    },
                );
                exists.insert(file_id.clone(), true);
                expected_visible.insert(file_id.clone(), Some(rev_id.clone()));
                (ChangelistAction::Create, Some(rev_id))
            } else {
                // 已存在：根据 i 的取值伪随机选择修改或删除
                if i % 3 == 0 {
                    // 删除：只更新可见性为 None，revision_id 在算法中不会被读取
                    exists.insert(file_id.clone(), false);
                    expected_visible.insert(file_id.clone(), None);
                    (ChangelistAction::Delete, None)
                } else {
                    // 修改：生成新的 revision
                    let counter = rev_counters.entry(file_id.clone()).or_insert(0);
                    *counter += 1;
                    let rev_id = format!("{}_r{}", file_id, counter);
                    let parent_rev_id = if *counter > 1 {
                        format!("{}_r{}", file_id, *counter - 1)
                    } else {
                        String::new()
                    };
                    revs.insert(
                        rev_id.clone(),
                        FileRevisionDoc {
                            id: rev_id.clone(),
                            branch_id: "branch_rand".to_string(),
                            file_id: file_id.clone(),
                            changelist_id: i,
                            binary_id: vec![format!("blob_{rev_id}")],
                            parent_revision_id: parent_rev_id,
                            size: 10 * i,
                            is_delete: false,
                            created_at: i,
                            metadata: FileRevisionMetadata {
                                file_mode: "755".to_string(),
                                hash: format!("h_{rev_id}"),
                                is_binary: false,
                                language: "txt".to_string(),
                            },
                        },
                    );
                    expected_visible.insert(file_id.clone(), Some(rev_id.clone()));
                    (ChangelistAction::Modify, Some(rev_id))
                }
            };

            let revision_str = revision_id_opt.clone().unwrap_or_else(|| format!("{file_id}_del{i}"));

            cls.insert(
                i,
                ChangelistDoc {
                    id: i,
                    parent_changelist_id: parent_id,
                    branch_id: "branch_rand".to_string(),
                    author: "userLong".to_string(),
                    description: format!("cl {i}"),
                    changes: vec![ChangelistChange {
                        file: file_id,
                        action,
                        revision: revision_str,
                    }],
                    committed_at: i,
                    files_count: 1,
                    metadata: ChangelistMetadata { labels: vec![] },
                },
            );
        }

        let get_branch = move |id: &str| {
            if id == branch.id {
                Ok(Some(branch.clone()))
            } else {
                Ok(None)
            }
        };

        let cls_clone = cls.clone();
        let files_clone = files.clone();
        let revs_clone = revs.clone();

        let get_changelist = move |id: i64| Ok(cls_clone.get(&id).cloned());
        let get_file = move |id: &str| Ok(files_clone.get(id).cloned());
        let get_file_revision = move |id: &str| Ok(revs_clone.get(id).cloned());

        let tree = construct_tree_from_changelist(
            "branch_rand",
            "//src/module/...",
            10,
            get_branch,
            get_changelist,
            get_file,
            get_file_revision,
        )
        .expect("constructTreeFromHead should succeed on long random chain");

        // 打印生成的文件树，便于调试和观察结构
        println!("=== FileTree for branch_rand @ CL10 ===\n{:#?}", tree);

        // 将 FileTree 展平为 file_id -> revision_id 的映射
        let mut tree_files: HashMap<String, String> = HashMap::new();
        collect_files(&tree.nodes, &mut tree_files);

        // 只检查位于 //src/module/... 下的文件：fa、fb
        for fid in ["fa", "fb"] {
            match expected_visible.get(fid) {
                Some(Some(expected_rev)) => {
                    let actual = tree_files
                        .get(fid)
                        .unwrap_or_else(|| panic!("file {fid} should be visible"));
                    assert_eq!(
                        actual, expected_rev,
                        "file {fid} should have revision {expected_rev}, got {actual}"
                    );
                }
                Some(None) | None => {
                    assert!(
                        !tree_files.contains_key(fid),
                        "file {fid} should not be visible in tree"
                    );
                }
            }
        }

        // fc 不在通配路径下，无论是否可见，都不应出现在树中
        assert!(!tree_files.contains_key("fc"));
    }

    #[test]
    fn construct_tree_large_scale() {
        // 构造一个包含大量文件的 changelist，用于测试在较大文件树规模下的行为。
        // 一部分文件位于 //src/module/... 下（应被包含），一部分位于其他路径（应被过滤掉）。
        use crate::metadata::ChangelistDoc;

        let branch = BranchDoc {
            id: "branch_large".to_string(),
            created_at: 0,
            created_by: "userLarge".to_string(),
            head_changelist_id: 1,
            metadata: BranchMetadata {
                description: "large branch".to_string(),
            },
        };

        const FILE_COUNT: usize = 100;
        let mut files: HashMap<String, FileDoc> = HashMap::new();
        let mut revs: HashMap<String, FileRevisionDoc> = HashMap::new();
        let mut changes: Vec<ChangelistChange> = Vec::new();
        let mut expected_visible: HashMap<String, Option<String>> = HashMap::new();

        for i in 0..FILE_COUNT {
            let file_id = format!("f{i}");
            let rev_id = format!("rev_{i}");

            let is_module_file = i % 2 == 0;
            let path = if is_module_file {
                // 生成多层目录结构，增加树的深度和广度
                format!(
                    "//src/module/dir_{}/sub_{}/file_{}.txt",
                    i % 5,
                    i % 3,
                    i
                )
            } else {
                format!(
                    "//src/other/dir_{}/file_{}.txt",
                    i % 4,
                    i
                )
            };

            files.insert(
                file_id.clone(),
                FileDoc {
                    id: file_id.clone(),
                    path,
                    seen_on_branches: vec!["branch_large".to_string()],
                    created_at: 0,
                    metadata: FileMetadata {
                        first_introduced_by: "userLarge".to_string(),
                    },
                },
            );

            revs.insert(
                rev_id.clone(),
                FileRevisionDoc {
                    id: rev_id.clone(),
                    branch_id: "branch_large".to_string(),
                    file_id: file_id.clone(),
                    changelist_id: 1,
                    binary_id: vec![format!("blob_{rev_id}")],
                    parent_revision_id: String::new(),
                    size: 100 + i as i64,
                    is_delete: false,
                    created_at: 1,
                    metadata: FileRevisionMetadata {
                        file_mode: "644".to_string(),
                        hash: format!("h_{rev_id}"),
                        is_binary: false,
                        language: "txt".to_string(),
                    },
                },
            );

            changes.push(ChangelistChange {
                file: file_id.clone(),
                action: ChangelistAction::Create,
                revision: rev_id.clone(),
            });

            expected_visible.insert(file_id, Some(rev_id));
        }

        let cl = ChangelistDoc {
            id: 1,
            parent_changelist_id: 0,
            branch_id: "branch_large".to_string(),
            author: "userLarge".to_string(),
            description: "large create".to_string(),
            changes,
            committed_at: 1,
            files_count: FILE_COUNT as i64,
            metadata: ChangelistMetadata { labels: vec![] },
        };

        let get_branch = move |id: &str| {
            if id == branch.id {
                Ok(Some(branch.clone()))
            } else {
                Ok(None)
            }
        };

        let files_clone = files.clone();
        let revs_clone = revs.clone();
        let cl_clone = cl.clone();

        let get_changelist = move |id: i64| {
            if id == 1 {
                Ok(Some(cl_clone.clone()))
            } else {
                Ok(None)
            }
        };
        let get_file = move |id: &str| Ok(files_clone.get(id).cloned());
        let get_file_revision = move |id: &str| Ok(revs_clone.get(id).cloned());

        let tree = construct_tree_from_changelist(
            "branch_large",
            "//src/module/...",
            1,
            get_branch,
            get_changelist,
            get_file,
            get_file_revision,
        )
        .expect("constructTreeFromHead should succeed on large scale");

        // 打印大规模文件树，便于观察目录结构与节点数量
        println!("=== FileTree for branch_large @ CL1 (large scale) ===\n{:#?}", tree);

        let mut tree_files: HashMap<String, String> = HashMap::new();
        collect_files(&tree.nodes, &mut tree_files);

        // 统计理论上应当出现在树中的文件（路径在 //src/module/... 下）
        let mut expected_module_files = 0usize;
        for (file_id, rev_opt) in &expected_visible {
            let file = files.get(file_id).expect("file must exist");
            let is_module_path = file.path.starts_with("//src/module/");
            if is_module_path {
                expected_module_files += 1;
                if let Some(expected_rev) = rev_opt {
                    let actual = tree_files
                        .get(file_id)
                        .unwrap_or_else(|| panic!("file {file_id} should be visible"));
                    assert_eq!(
                        actual, expected_rev,
                        "file {file_id} should have revision {expected_rev}, got {actual}"
                    );
                }
            } else {
                // 非 module 路径文件不应该出现在树中
                assert!(
                    !tree_files.contains_key(file_id),
                    "non-module file {file_id} should not be in tree"
                );
            }
        }

        // 树中文件数量应与 module 文件数量一致，规模应明显大于之前的小规模测试
        assert_eq!(tree_files.len(), expected_module_files);
        assert!(expected_module_files > 10); // 简单检查规模足够大
    }
}

