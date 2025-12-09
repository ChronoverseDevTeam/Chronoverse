pub const HASH_SIZE: usize = 32;

pub const PACK_MAGIC: u32 = 0x4352_5642; // "CRVB"
pub const PACK_VERSION: u16 = 0x0001;
pub const PACK_HEADER_SIZE: u64 = 10;
pub const PACK_ENTRY_FIXED_SECTION: u64 = 38; // len(4) + flags(2) + hash(32)
pub const PACK_TRAILER_SIZE: u64 = 4; // CRC32

pub const INDEX_MAGIC: u32 = 0x4352_5649; // "CRVI"
pub const INDEX_VERSION: u16 = 0x0001;
pub const INDEX_HEADER_SIZE: u64 = 18;
pub const INDEX_ENTRY_SIZE: u64 = 46; // hash + offset + length + flags
pub const INDEX_TRAILER_SIZE: u64 = 4;

pub const SHARD_DIR_PREFIX: &str = "shard-";
pub const PACK_FILE_PREFIX: &str = "pack-";
pub const PACK_DATA_SUFFIX: &str = ".dat";
pub const PACK_INDEX_SUFFIX: &str = ".idx";
