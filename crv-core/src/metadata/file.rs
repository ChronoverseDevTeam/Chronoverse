use crate::metadata::file_revision::MetaFileRevision;
use serde::{self, Deserialize, Serialize};
use std::hash::{Hash, Hasher};

// Only used in edge, not exists in mongo db
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaFile {
    #[serde(rename = "_id")]
    pub depot_path: String,
    pub locked_by: String,
    pub revisions: Vec<MetaFileRevision>,
}

impl PartialEq for MetaFile {
    fn eq(&self, other: &Self) -> bool {
        self.depot_path == other.depot_path
    }
}

impl Eq for MetaFile {}

impl Hash for MetaFile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.depot_path.hash(state);
    }
}
