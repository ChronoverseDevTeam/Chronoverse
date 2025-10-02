
pub struct ChangelistMetadata {
    /// changelist 的唯一标识
    pub id: u32,
    /// 该 changelist 中所有文件的路径列表
    pub file_paths: Vec<String>,
    pub desc: String
}

impl ChangelistMetadata {
    pub fn new(id: u32, file_paths: Vec<String>, desc: String) -> Self {
        ChangelistMetadata { id, file_paths, desc }
    }
}
