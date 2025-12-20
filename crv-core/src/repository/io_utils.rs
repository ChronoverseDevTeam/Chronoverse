use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration};

use blake3::Hasher as Blake3Hasher;
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

/// 计算一段字节数据的 Blake3 哈希值，返回 32 字节原始哈希。
pub fn compute_blake3_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Blake3Hasher::new();
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

/// 计算字符串的 Blake3 哈希值（对 UTF-8 字节进行哈希），返回 32 字节原始哈希。
pub fn compute_blake3_str(s: &str) -> [u8; 32] {
    compute_blake3_bytes(s.as_bytes())
}

/// 将 32 字节的 Blake3 哈希值转化为十六进制字符串（小写）。
pub fn blake3_hash_to_hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 将 64 位十六进制字符串解析为 Blake3 哈希字节数组（32 字节）。
///
/// - `s`：十六进制字符串，允许大小写，前后会自动 `trim()`；
/// - 返回 `Some([u8; 32])` 表示解析成功，`None` 表示长度非法或包含非 hex 字符。
pub fn blake3_hex_to_hash(s: &str) -> Option<[u8; 32]> {
    let hex = s.trim();
    if hex.len() != 64 {
        return None;
    }

    let mut out = [0u8; 32];
    for i in 0..32 {
        let idx = i * 2;
        let byte_str = &hex[idx..idx + 2];
        let byte = u8::from_str_radix(byte_str, 16).ok()?;
        out[i] = byte;
    }
    Some(out)
}

/// 针对 bytes 的流式 Blake3 哈希计算器。
///
/// 用法示例：
/// ```ignore
/// let mut hasher = Blake3Stream::new();
/// hasher.update(part1);
/// hasher.update(part2);
/// let hash = hasher.finalize();
/// ```
#[derive(Debug, Clone)]
pub struct Blake3Stream {
    inner: Blake3Hasher,
}

impl Blake3Stream {
    /// 创建一个新的流式 Blake3 计算器。
    pub fn new() -> Self {
        Self {
            inner: Blake3Hasher::new(),
        }
    }

    /// 追加一段字节数据到哈希计算中。
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// 计算并返回最终的 32 字节哈希值。
    ///
    /// 调用后内部状态仍可继续 `update`，相当于对当前状态做一次快照。
    pub fn finalize(&self) -> [u8; 32] {
        *self.inner.finalize().as_bytes()
    }
}

/// 简单文件锁：通过 create_new 的锁文件实现跨进程互斥，支持过期重试。
/// 注意：崩溃时可能留下锁文件，依靠过期时间进行清理。
#[derive(Debug)]
pub struct FileLockGuard {
    path: PathBuf,
    _file: File,
}

impl FileLockGuard {
    pub fn acquire(
        path: impl AsRef<Path>,
        retry: usize,
        backoff: Duration,
        stale_after: Duration,
    ) -> io::Result<Self> {
        let path = path.as_ref();
        ensure_parent_dir(path)?;
        for attempt in 0..=retry {
            match try_create_lock_file(path) {
                Ok(file) => {
                    return Ok(Self {
                        path: path.to_path_buf(),
                        _file: file,
                    })
                }
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    // 检查过期
                    if is_stale_lock(path, stale_after)? {
                        let _ = fs::remove_file(path);
                        continue;
                    }
                    if attempt == retry {
                        return Err(err);
                    }
                    thread::sleep(backoff);
                }
                Err(err) => return Err(err),
            }
        }
        unreachable!("retry loop must return or error earlier");
    }
}

impl Drop for FileLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn try_create_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

fn is_stale_lock(path: &Path, stale_after: Duration) -> io::Result<bool> {
    let metadata = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) => return Err(err),
    };
    match metadata.modified() {
        Ok(time) => Ok(time
            .elapsed()
            .map(|elapsed| elapsed > stale_after)
            .unwrap_or(false)),
        Err(_) => Ok(false),
    }
}

