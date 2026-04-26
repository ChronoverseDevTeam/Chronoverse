use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Serialize;
use thiserror::Error;

use crate::crv2::postgres::{
    dao::{self, DaoError},
    entity::file_revision::Model as FileRevisionModel,
    executor::PostgreExecutor,
};

#[derive(Debug, Serialize)]
pub struct DepotBrowseResponse {
    pub path: String,
    pub changelist_id: i64,
    pub recursive: bool,
    pub node: DepotNode,
}

#[derive(Debug, Serialize)]
#[serde(tag = "node_type", rename_all = "snake_case")]
pub enum DepotNode {
    Directory {
        path: String,
        name: String,
        children: Vec<DepotNode>,
    },
    File {
        path: String,
        name: String,
        generation: i64,
        revision: i64,
        changelist_id: i64,
        chunk_hashes: Vec<String>,
        size: i64,
        revision_created_at: i64,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathHistoryType {
    File,
    Directory,
}

#[derive(Debug, Serialize)]
pub struct FileHistoryEntry {
    pub generation: i64,
    pub revision: i64,
    pub changelist_id: i64,
    pub chunk_hashes: Vec<String>,
    pub size: i64,
    pub is_deletion: bool,
    pub revision_created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct DirectoryHistoryEntry {
    pub changelist_id: i64,
    pub author: String,
    pub description: String,
    pub committed_at: i64,
    pub changed_paths_count: usize,
}

#[derive(Debug, Serialize)]
pub struct PathHistoryResponse {
    pub path: String,
    pub path_type: PathHistoryType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_revisions: Option<Vec<FileHistoryEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changelists: Option<Vec<DirectoryHistoryEntry>>,
}

#[derive(Debug, Error)]
pub enum DepotServiceError {
    #[error("path must not be empty")]
    EmptyPath,

    #[error("invalid depot path: {0}")]
    InvalidPath(String),

    #[error("changelist must be positive: {0}")]
    InvalidChangelist(i64),

    #[error("invalid changelist range: from {from_id} > to {to_id}")]
    InvalidChangelistRange { from_id: i64, to_id: i64 },

    #[error("changelist not found: {0}")]
    ChangelistNotFound(i64),

    #[error("path not found at changelist {changelist_id}: {path}")]
    PathNotFoundAtChangelist { path: String, changelist_id: i64 },

    #[error("path not found: {0}")]
    PathNotFound(String),

    #[error("dao error: {0}")]
    Dao(#[from] DaoError),
}

pub async fn browse_depot_tree(
    pg: &PostgreExecutor,
    path: &str,
    changelist_id: i64,
    recursive: bool,
) -> Result<DepotBrowseResponse, DepotServiceError> {
    let path = normalize_depot_path(path)?;
    validate_changelist_id(changelist_id)?;
    ensure_changelist_exists(pg, changelist_id).await?;

    if let Some(revision) = dao::file_revision::find_latest_at_changelist(pg.connection(), &path, changelist_id).await? {
        if !revision.is_deletion {
            return Ok(DepotBrowseResponse {
                path: path.clone(),
                changelist_id,
                recursive,
                node: file_node(&path, &revision),
            });
        }
    }

    let prefix = directory_prefix(&path);
    let files = dao::file::find_by_prefix(pg.connection(), &prefix).await?;
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let latest = dao::file_revision::find_latest_for_paths_at(pg.connection(), &paths, changelist_id).await?;

    let mut active_entries: Vec<(String, FileRevisionModel)> = latest
        .into_iter()
        .filter(|(_, revision)| !revision.is_deletion)
        .collect();
    active_entries.sort_by(|left, right| left.0.cmp(&right.0));

    if active_entries.is_empty() {
        return Err(DepotServiceError::PathNotFoundAtChangelist { path, changelist_id });
    }

    let node = if recursive {
        build_recursive_directory_node(&path, &active_entries)
    } else {
        build_shallow_directory_node(&path, &active_entries)
    };

    Ok(DepotBrowseResponse {
        path,
        changelist_id,
        recursive,
        node,
    })
}

pub async fn query_path_history(
    pg: &PostgreExecutor,
    path: &str,
    from_changelist: Option<i64>,
    to_changelist: Option<i64>,
    limit: Option<usize>,
) -> Result<PathHistoryResponse, DepotServiceError> {
    let path = normalize_depot_path(path)?;
    validate_history_bounds(from_changelist, to_changelist)?;

    if dao::file::find_by_path(pg.connection(), &path).await?.is_some() {
        let revisions = dao::file_revision::find_all_by_path(pg.connection(), &path).await?;
        let mut revisions: Vec<FileHistoryEntry> = revisions
            .into_iter()
            .filter(|revision| matches_changelist_range(revision.changelist_id, from_changelist, to_changelist))
            .map(|revision| FileHistoryEntry {
                generation: revision.generation,
                revision: revision.revision,
                changelist_id: revision.changelist_id,
                chunk_hashes: revision.chunk_hash_list(),
                size: revision.size,
                is_deletion: revision.is_deletion,
                revision_created_at: revision.created_at,
            })
            .collect();
        revisions.sort_by(|left, right| {
            right
                .changelist_id
                .cmp(&left.changelist_id)
                .then_with(|| right.generation.cmp(&left.generation))
                .then_with(|| right.revision.cmp(&left.revision))
        });
        if let Some(limit) = limit {
            revisions.truncate(limit);
        }

        return Ok(PathHistoryResponse {
            path,
            path_type: PathHistoryType::File,
            file_revisions: Some(revisions),
            changelists: None,
        });
    }

    let prefix = directory_prefix(&path);
    let file_rows = dao::file::find_by_prefix(pg.connection(), &prefix).await?;
    if file_rows.is_empty() {
        return Err(DepotServiceError::PathNotFound(path));
    }

    let revisions = dao::file_revision::find_by_prefix_in_range(
        pg.connection(),
        &prefix,
        from_changelist,
        to_changelist,
    )
    .await?;

    let mut changelist_to_paths: HashMap<i64, HashSet<String>> = HashMap::new();
    for revision in revisions {
        changelist_to_paths
            .entry(revision.changelist_id)
            .or_default()
            .insert(revision.path);
    }

    let mut changelist_ids: Vec<i64> = changelist_to_paths.keys().copied().collect();
    changelist_ids.sort_by(|left, right| right.cmp(left));
    if let Some(limit) = limit {
        changelist_ids.truncate(limit);
    }

    let mut changelists = Vec::with_capacity(changelist_ids.len());
    for changelist_id in changelist_ids {
        let changelist = dao::changelist::find_by_id(pg.connection(), changelist_id)
            .await?
            .ok_or(DepotServiceError::ChangelistNotFound(changelist_id))?;
        let changed_paths_count = changelist_to_paths
            .get(&changelist_id)
            .map(HashSet::len)
            .unwrap_or_default();
        changelists.push(DirectoryHistoryEntry {
            changelist_id,
            author: changelist.author,
            description: changelist.description,
            committed_at: changelist.committed_at,
            changed_paths_count,
        });
    }

    Ok(PathHistoryResponse {
        path,
        path_type: PathHistoryType::Directory,
        file_revisions: None,
        changelists: Some(changelists),
    })
}

fn validate_changelist_id(changelist_id: i64) -> Result<(), DepotServiceError> {
    if changelist_id <= 0 {
        return Err(DepotServiceError::InvalidChangelist(changelist_id));
    }
    Ok(())
}

fn validate_history_bounds(
    from_changelist: Option<i64>,
    to_changelist: Option<i64>,
) -> Result<(), DepotServiceError> {
    if let Some(changelist_id) = from_changelist {
        validate_changelist_id(changelist_id)?;
    }
    if let Some(changelist_id) = to_changelist {
        validate_changelist_id(changelist_id)?;
    }
    if let (Some(from_id), Some(to_id)) = (from_changelist, to_changelist) {
        if from_id > to_id {
            return Err(DepotServiceError::InvalidChangelistRange { from_id, to_id });
        }
    }
    Ok(())
}

async fn ensure_changelist_exists(
    pg: &PostgreExecutor,
    changelist_id: i64,
) -> Result<(), DepotServiceError> {
    if dao::changelist::find_by_id(pg.connection(), changelist_id)
        .await?
        .is_none()
    {
        return Err(DepotServiceError::ChangelistNotFound(changelist_id));
    }
    Ok(())
}

fn normalize_depot_path(path: &str) -> Result<String, DepotServiceError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(DepotServiceError::EmptyPath);
    }
    if !path.starts_with("//") {
        return Err(DepotServiceError::InvalidPath(path.to_owned()));
    }
    if path == "//" {
        return Ok(path.to_owned());
    }

    let normalized = path.trim_end_matches('/');
    if normalized.len() < 3 {
        return Err(DepotServiceError::InvalidPath(path.to_owned()));
    }

    Ok(normalized.to_owned())
}

fn directory_prefix(path: &str) -> String {
    if path == "//" {
        return "//".to_owned();
    }
    format!("{}/", path)
}

fn node_name(path: &str) -> String {
    if path == "//" {
        return "//".to_owned();
    }
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

fn file_node(path: &str, revision: &FileRevisionModel) -> DepotNode {
    DepotNode::File {
        path: path.to_owned(),
        name: node_name(path),
        generation: revision.generation,
        revision: revision.revision,
        changelist_id: revision.changelist_id,
        chunk_hashes: revision.chunk_hash_list(),
        size: revision.size,
        revision_created_at: revision.created_at,
    }
}

#[derive(Default)]
struct DirectoryBuilder {
    directories: BTreeMap<String, DirectoryBuilder>,
    files: BTreeMap<String, DepotNode>,
}

fn build_recursive_directory_node(
    root_path: &str,
    entries: &[(String, FileRevisionModel)],
) -> DepotNode {
    let mut root = DirectoryBuilder::default();

    for (path, revision) in entries {
        let relative = relative_segments(root_path, path);
        if relative.is_empty() {
            continue;
        }

        let mut current = &mut root;
        for segment in &relative[..relative.len() - 1] {
            current = current
                .directories
                .entry((*segment).to_owned())
                .or_insert_with(DirectoryBuilder::default);
        }
        current
            .files
            .insert(relative.last().unwrap().to_string(), file_node(path, revision));
    }

    DepotNode::Directory {
        path: root_path.to_owned(),
        name: node_name(root_path),
        children: directory_builder_to_nodes(root_path, root),
    }
}

fn build_shallow_directory_node(
    root_path: &str,
    entries: &[(String, FileRevisionModel)],
) -> DepotNode {
    let mut files = BTreeMap::new();
    let mut directories = BTreeMap::new();

    for (path, revision) in entries {
        let relative = relative_segments(root_path, path);
        if relative.is_empty() {
            continue;
        }

        if relative.len() == 1 {
            files.insert(relative[0].to_owned(), file_node(path, revision));
            continue;
        }

        let child_name = relative[0];
        directories.entry(child_name.to_owned()).or_insert_with(|| DepotNode::Directory {
            path: join_child_path(root_path, child_name),
            name: child_name.to_owned(),
            children: Vec::new(),
        });
    }

    let mut children: Vec<DepotNode> = directories.into_values().collect();
    children.extend(files.into_values());

    DepotNode::Directory {
        path: root_path.to_owned(),
        name: node_name(root_path),
        children,
    }
}

fn directory_builder_to_nodes(root_path: &str, builder: DirectoryBuilder) -> Vec<DepotNode> {
    let mut nodes = Vec::new();

    for (name, child) in builder.directories {
        let child_path = join_child_path(root_path, &name);
        nodes.push(DepotNode::Directory {
            path: child_path.clone(),
            name,
            children: directory_builder_to_nodes(&child_path, child),
        });
    }

    for file in builder.files.into_values() {
        nodes.push(file);
    }

    nodes
}

fn join_child_path(root_path: &str, child_name: &str) -> String {
    if root_path == "//" {
        format!("//{child_name}")
    } else {
        format!("{root_path}/{child_name}")
    }
}

fn relative_segments<'a>(root_path: &str, child_path: &'a str) -> Vec<&'a str> {
    let relative = if root_path == "//" {
        child_path.trim_start_matches("//")
    } else {
        child_path
            .strip_prefix(&format!("{root_path}/"))
            .unwrap_or_default()
    };

    relative
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn matches_changelist_range(
    changelist_id: i64,
    from_changelist: Option<i64>,
    to_changelist: Option<i64>,
) -> bool {
    if let Some(from_id) = from_changelist {
        if changelist_id < from_id {
            return false;
        }
    }
    if let Some(to_id) = to_changelist {
        if changelist_id > to_id {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        DepotNode, build_recursive_directory_node, build_shallow_directory_node,
        normalize_depot_path,
    };
    use crate::crv2::postgres::entity::file_revision::Model as FileRevisionModel;

    #[test]
    fn normalize_depot_path_trims_directory_suffix() {
        let normalized = normalize_depot_path("//depot/main/src/").expect("path should normalize");

        assert_eq!(normalized, "//depot/main/src");
    }

    #[test]
    fn normalize_depot_path_rejects_non_depot_paths() {
        let error = normalize_depot_path("depot/main/src").expect_err("path should be rejected");

        assert_eq!(error.to_string(), "invalid depot path: depot/main/src");
    }

    #[test]
    fn build_shallow_directory_node_keeps_only_direct_children() {
        let entries = vec![
            (
                "//depot/main/src/lib.rs".to_owned(),
                revision_model("//depot/main/src/lib.rs", 11),
            ),
            (
                "//depot/main/src/nested/mod.rs".to_owned(),
                revision_model("//depot/main/src/nested/mod.rs", 12),
            ),
        ];

        let node = build_shallow_directory_node("//depot/main/src", &entries);

        let DepotNode::Directory { children, .. } = node else {
            panic!("expected directory node");
        };

        assert_eq!(children.len(), 2);
        assert!(children.iter().any(|child| matches!(
            child,
            DepotNode::File { path, .. } if path == "//depot/main/src/lib.rs"
        )));
        assert!(children.iter().any(|child| matches!(
            child,
            DepotNode::Directory { path, children, .. }
                if path == "//depot/main/src/nested" && children.is_empty()
        )));
    }

    #[test]
    fn build_recursive_directory_node_nests_descendants() {
        let entries = vec![
            (
                "//depot/main/src/nested/mod.rs".to_owned(),
                revision_model("//depot/main/src/nested/mod.rs", 21),
            ),
        ];

        let node = build_recursive_directory_node("//depot/main/src", &entries);

        let DepotNode::Directory { children, .. } = node else {
            panic!("expected directory node");
        };

        let nested = children
            .into_iter()
            .find(|child| matches!(child, DepotNode::Directory { path, .. } if path == "//depot/main/src/nested"))
            .expect("nested directory should exist");

        let DepotNode::Directory { children, .. } = nested else {
            panic!("expected nested directory node");
        };
        assert!(children.iter().any(|child| matches!(
            child,
            DepotNode::File { path, .. } if path == "//depot/main/src/nested/mod.rs"
        )));
    }

    fn revision_model(path: &str, changelist_id: i64) -> FileRevisionModel {
        FileRevisionModel {
            path: path.to_owned(),
            generation: 1,
            revision: 1,
            changelist_id,
            chunk_hashes: json!(["abc123"]),
            size: 128,
            is_deletion: false,
            created_at: 1_700_000_000_000,
        }
    }
}