pub mod file_block;

use crate::storage::file_block::FileBlock;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Options to control chunking behaviors.
#[derive(Debug, Clone)]
pub struct ChunkingOptions {
    /// Fixed block size for large files.
    pub fixed_block_size: usize,
    /// Threshold to treat a file as small; small files use CDC.
    pub small_file_threshold: usize,
    /// CDC window size in bytes.
    pub cdc_window_size: usize,
    /// CDC minimum chunk size.
    pub cdc_min_size: usize,
    /// CDC target/average chunk size (ideally a power of two).
    pub cdc_avg_size: usize,
    /// CDC maximum chunk size.
    pub cdc_max_size: usize,
}

impl Default for ChunkingOptions {
    fn default() -> Self {
        let four_mib = 4 * 1024 * 1024;
        Self {
            fixed_block_size: four_mib,
            small_file_threshold: four_mib,
            cdc_window_size: 48,
            cdc_min_size: 8 * 1024,
            cdc_avg_size: 32 * 1024,
            cdc_max_size: 64 * 1024,
        }
    }
}

/// Split a file and persist blocks under `store_root` using 3-level directory layout.
/// Returns the list of `FileBlock`s (with ids computed by blake3).
pub fn chunk_and_store_file<P: AsRef<Path>, Q: AsRef<Path>>(
    source_file: P,
    store_root: Q,
    options: &ChunkingOptions,
) -> std::io::Result<Vec<FileBlock>> {
    let meta = fs::metadata(&source_file)?;
    let file_len = meta.len() as usize;
    let mut blocks: Vec<FileBlock> = Vec::new();

    if file_len == 0 {
        // Represent empty file as a single empty block
        let block = FileBlock::from_bytes(Vec::new());
        persist_block_if_needed(&block, &store_root)?;
        blocks.push(block);
        return Ok(blocks);
    }

    if file_len <= options.small_file_threshold {
        let mut buf = Vec::with_capacity(file_len);
        File::open(&source_file)?.read_to_end(&mut buf)?;
        let ranges = cdc_chunk_ranges(&buf, options);
        for (start, end) in ranges {
            let block = FileBlock::from_bytes(buf[start..end].to_vec());
            persist_block_if_needed(&block, &store_root)?;
            blocks.push(block);
        }
    } else {
        blocks.extend(chunk_and_store_fixed(&source_file, &store_root, options)?);
    }
    Ok(blocks)
}

/// Fixed-size chunking for large files with streaming IO.
fn chunk_and_store_fixed<P: AsRef<Path>, Q: AsRef<Path>>(
    source_file: P,
    store_root: Q,
    options: &ChunkingOptions,
) -> std::io::Result<Vec<FileBlock>> {
    let mut file = File::open(&source_file)?;
    let mut blocks: Vec<FileBlock> = Vec::new();
    let mut buf = vec![0u8; options.fixed_block_size];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        let block = FileBlock::from_bytes(buf[..read].to_vec());
        persist_block_if_needed(&block, &store_root)?;
        blocks.push(block);
    }
    Ok(blocks)
}

/// Compute CDC chunk ranges for a small file using a buzhash-like rolling hash.
/// Returns a vector of (start, end) byte indices, end-exclusive.
fn cdc_chunk_ranges(bytes: &[u8], options: &ChunkingOptions) -> Vec<(usize, usize)> {
    let len = bytes.len();
    if len == 0 {
        return vec![(0, 0)];
    }

    let min_size = options.cdc_min_size.min(len);
    let max_size = options.cdc_max_size.min(len);
    let avg_pow2 = options.cdc_avg_size.next_power_of_two();
    let mask: u64 = (avg_pow2 as u64) - 1;
    let w = options.cdc_window_size.min(len.max(1));

    // Initialize random table deterministically to avoid large static.
    let table = build_gear_table();

    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    let mut h: u64 = 0;

    // Warm up initial window
    let warm = w.min(len);
    for i in 0..warm {
        h = rotl64(h, 1) ^ table[bytes[i] as usize];
    }

    let mut i = warm;
    while i < len {
        let since = i - start;
        let at_max = since >= max_size;
        let can_cut = since >= min_size && ((h & mask) == 0);
        if at_max || can_cut {
            ranges.push((start, i));
            start = i;
        }

        // Advance rolling hash by one byte
        let incoming = bytes[i] as usize;
        let outgoing = if i >= w {
            bytes[i - w] as usize
        } else {
            0usize
        };
        let out_rot = rotl64(table[outgoing], (w % 64) as u32);
        h = rotl64(h, 1) ^ table[incoming] ^ out_rot;
        i += 1;
    }

    // Push the tail
    if start < len {
        ranges.push((start, len));
    }
    ranges
}

fn rotl64(x: u64, r: u32) -> u64 {
    (x << r) | (x >> (64 - r))
}

fn build_gear_table() -> [u64; 256] {
    // XorShift64* PRNG for deterministic table
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ nanos_seed();
    let mut table = [0u64; 256];
    for i in 0..256 {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        table[i] = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    }
    table
}

fn nanos_seed() -> u64 {
    // A stable but varying bit-mix based on process start time.
    // Not security-critical; only needs to be deterministic per-process.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.as_nanos() as u64;
    nanos.rotate_left(13) ^ 0xD6E8_FD50_88CC_AA27
}

/// Persist a block under 3-level directory based on its hex id.
fn persist_block_if_needed<P: AsRef<Path>>(
    block: &FileBlock,
    store_root: P,
) -> std::io::Result<PathBuf> {
    let id = &block.id;
    let p1 = &id[0..2];
    let p2 = &id[2..4];
    let p3 = &id[4..6];
    let mut dir = PathBuf::from(store_root.as_ref());
    dir.push(p1);
    dir.push(p2);
    dir.push(p3);
    fs::create_dir_all(&dir)?;
    let mut path = dir;
    path.push(id);

    if path.exists() {
        return Ok(path);
    }

    let mut f = File::create(&path)?;
    f.write_all(&block.block_data)?;
    Ok(path)
}
