use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use crc32fast::Hasher as Crc32;

use super::error::Result;

const IO_BUFFER_SIZE: usize = 64 * 1024;

pub fn ensure_parent_dir(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

pub fn compute_crc32(file: &File, len: u64) -> Result<u32> {
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    let mut remaining = len;
    let mut buf = vec![0u8; IO_BUFFER_SIZE];
    let mut hasher = Crc32::new();
    while remaining > 0 {
        let to_read = remaining.min(buf.len() as u64) as usize;
        reader.read_exact(&mut buf[..to_read])?;
        hasher.update(&buf[..to_read]);
        remaining -= to_read as u64;
    }
    Ok(hasher.finalize())
}
