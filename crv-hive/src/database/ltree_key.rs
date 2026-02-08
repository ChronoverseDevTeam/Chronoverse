use crv_core::path::basic::DepotPath;
use thiserror::Error;

/// 将 `DepotPath` 编码成可用于 Postgres `ltree` 的 key。
///
/// ## 为什么需要编码
/// `ltree` 的 label 仅允许 `[A-Za-z0-9_]`，而 depot path/文件名允许中文、空格、点号、短横线等。
/// 因此不能直接把原始 depot path 写进 `ltree` 列（会失败或层级语义被 `.` 打散）。
///
/// ## 编码规则（稳定、可逆、可做 CHECK 约束）
/// - 每个 path segment（目录名与文件名）按 UTF-8 字节序列做 **小写 hex**；
/// - 用 `.` 连接为 `ltree` 的层级分隔符；
/// - 解码时再逐段 hex -> bytes -> UTF-8 还原。
///
/// 注意：这是“数据库 key”，不是展示用字符串。展示时请用解码后的 depot path。
#[derive(Debug, Error)]
pub enum LtreeKeyError {
    #[error("invalid depot path: {0}")]
    InvalidDepotPath(String),

    #[error("invalid ltree key: {0}")]
    InvalidLtreeKey(String),

    #[error("segment too long after encoding (len={encoded_len}): {segment}")]
    SegmentTooLong { segment: String, encoded_len: usize },
}

/// 由 depot path 字符串（形如 `//a/b/c.txt`）生成 `ltree` key。
pub fn depot_path_str_to_ltree_key(path: &str) -> Result<String, LtreeKeyError> {
    let depot =
        DepotPath::parse(path).map_err(|e| LtreeKeyError::InvalidDepotPath(e.to_string()))?;
    depot_path_to_ltree_key(&depot)
}

/// 由 `DepotPath` 生成 `ltree` key。
pub fn depot_path_to_ltree_key(depot: &DepotPath) -> Result<String, LtreeKeyError> {
    let mut labels = Vec::with_capacity(depot.dirs.len() + 1);
    for seg in depot.dirs.iter().chain(std::iter::once(&depot.file)) {
        let encoded = hex_lower(seg.as_bytes());
        // Postgres ltree label 最大 256 字符（byte）。hex 编码会膨胀 2 倍，因此需要显式保护。
        if encoded.len() > 256 {
            return Err(LtreeKeyError::SegmentTooLong {
                segment: seg.clone(),
                encoded_len: encoded.len(),
            });
        }
        labels.push(encoded);
    }
    Ok(labels.join("."))
}

/// 将目录或通配路径（`//a/b/` 或 `//a/b/...` 或 `//...`）编码为 ltree 前缀。
///
/// - 返回空字符串表示根前缀（`//...`）。
pub fn depot_dir_or_wildcard_to_ltree_prefix(path: &str) -> Result<String, LtreeKeyError> {
    let mut raw = path.trim();
    if !raw.starts_with("//") {
        return Err(LtreeKeyError::InvalidDepotPath("must start with '//'".to_string()));
    }

    if raw.ends_with("...") {
        raw = raw.trim_end_matches("...");
    }
    raw = raw.trim_end_matches('/');

    let rest = raw.trim_start_matches("//");
    if rest.is_empty() {
        return Ok(String::new());
    }

    let mut labels = Vec::new();
    for seg in rest.split('/') {
        if seg.is_empty() {
            return Err(LtreeKeyError::InvalidDepotPath("empty depot path".to_string()));
        }
        let encoded = hex_lower(seg.as_bytes());
        if encoded.len() > 256 {
            return Err(LtreeKeyError::SegmentTooLong {
                segment: seg.to_string(),
                encoded_len: encoded.len(),
            });
        }
        labels.push(encoded);
    }

    Ok(labels.join("."))
}

/// 将 `ltree` key 还原成 depot path 字符串（形如 `//a/b/c.txt`）。
pub fn ltree_key_to_depot_path_str(key: &str) -> Result<String, LtreeKeyError> {
    if key.trim().is_empty() {
        return Err(LtreeKeyError::InvalidLtreeKey("empty".to_string()));
    }

    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() {
        return Err(LtreeKeyError::InvalidLtreeKey("no labels".to_string()));
    }

    let mut decoded: Vec<String> = Vec::with_capacity(parts.len());
    for p in parts {
        let bytes = hex_decode_lower(p)
            .ok_or_else(|| LtreeKeyError::InvalidLtreeKey(format!("bad hex label: {p}")))?;
        let s = String::from_utf8(bytes)
            .map_err(|_| LtreeKeyError::InvalidLtreeKey(format!("non-utf8 label: {p}")))?;
        decoded.push(s);
    }

    if decoded.len() == 1 {
        return Ok(format!("//{}", decoded[0]));
    }

    let file = decoded
        .pop()
        .ok_or_else(|| LtreeKeyError::InvalidLtreeKey("missing file".to_string()))?;
    Ok(format!("//{}/{}", decoded.join("/"), file))
}

fn hex_lower(bytes: &[u8]) -> String {
    // 预分配：hex 长度为 2 * bytes.len()
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(nibble_to_hex((b >> 4) & 0x0f));
        out.push(nibble_to_hex(b & 0x0f));
    }
    out
}

fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => unreachable!("nibble out of range"),
    }
}

fn hex_decode_lower(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.is_empty() || (s.len() % 2 != 0) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_val(bytes[i])?;
        let lo = hex_val(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_unicode_and_dots() {
        let raw = "//crv/cli/src/新建文本文档.txt";
        let key = depot_path_str_to_ltree_key(raw).expect("encode");
        // 约定：hex labels + dot 分隔，可用于 DB CHECK 约束
        assert!(key
            .chars()
            .all(|c| matches!(c, '0'..='9' | 'a'..='f' | '.' )));
        let decoded = ltree_key_to_depot_path_str(&key).expect("decode");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn roundtrip_with_dash_and_multi_ext() {
        let raw = "//opt/app-v1.2.3/config_prod.yaml";
        let key = depot_path_str_to_ltree_key(raw).expect("encode");
        let decoded = ltree_key_to_depot_path_str(&key).expect("decode");
        assert_eq!(decoded, raw);
    }
}


