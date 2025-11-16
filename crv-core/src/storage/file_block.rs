use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBlock {
    // Hash id here, should equal to file name
    pub id: String,
    pub hash_algorithm: String,
    pub block_data: Vec<u8>,
}

impl FileBlock {
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let hash = blake3::hash(&bytes);
        let id = hash.to_hex().to_string();
        Self {
            id,
            hash_algorithm: "blake3".to_string(),
            block_data: bytes,
        }
    }
}
