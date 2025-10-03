use crate::metadata::file_revision::MetaFileRevision;

// Only used in edge, not exists in mongo db
pub struct MetaFile {
    pub locked_by: String,
    pub depot_path: String,
    pub revisions: Vec<MetaFileRevision>
}