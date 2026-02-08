use crv_core::path::basic::{
    DepotPath as CoreDepotPath, DepotPathWildcard as CoreDepotPathWildcard, FilenameWildcard,
};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock, RwLock, Weak};
use thiserror::Error;

/// 允许驻留（intern）的最大 segment 数量（目录名/文件名段的去重集合大小）。
///
/// - **硬上限**：达到/超过时会按 LRU 淘汰旧 segment（从驻留表移除）。
/// - 淘汰不会影响已构建的 `DepotPath`：它们持有自己的 `Arc<str>`，字符串内存会随引用计数自然释放。
pub const MAX_SEGMENT_AMOUNT: usize = 100_000;

#[derive(Debug, Clone)]
struct InternEntry {
    weak: Weak<str>,
    last_access: u64,
}

#[derive(Debug, Default)]
struct SegmentInterner {
    map: HashMap<Arc<str>, InternEntry>,
    // 追加式 LRU 队列：允许重复项；淘汰时用 last_access 去重。
    lru: VecDeque<(u64, Arc<str>)>,
    clock: u64,
}

/// DepotPath 的全局管理器（单例）。
///
/// 负责：
/// - segment interning（相同目录名/文件名只存一份）
/// - 将 `DepotPath` 格式化为字符串（`//a/b/c.txt` / `//a/b/` / `//a/b/...`）
pub struct DepotPathManager {
    interner: RwLock<SegmentInterner>,
}

impl DepotPathManager {
    /// 创建一个新的 manager 实例（通常你只需要用 `global()` 单例）。
    pub fn new() -> Self {
        Self {
            interner: RwLock::new(SegmentInterner::default()),
        }
    }

    pub fn global() -> &'static DepotPathManager {
        static INSTANCE: OnceLock<DepotPathManager> = OnceLock::new();
        INSTANCE.get_or_init(DepotPathManager::new)
    }

    fn intern_one_locked(inner: &mut SegmentInterner, s: &str) -> Arc<str> {
        inner.clock = inner.clock.wrapping_add(1);
        let now = inner.clock;

        if let Some(entry) = inner.map.get_mut(s) {
            if let Some(upgraded) = entry.weak.upgrade() {
                entry.last_access = now;
                inner.lru.push_back((now, upgraded.clone()));
                return upgraded;
            }
            // 已过期：重新创建
        }

        let arc: Arc<str> = Arc::from(s);
        inner.map.insert(
            arc.clone(),
            InternEntry {
                weak: Arc::downgrade(&arc),
                last_access: now,
            },
        );
        inner.lru.push_back((now, arc.clone()));
        arc
    }

    fn evict_lru_locked(inner: &mut SegmentInterner) {
        while inner.map.len() > MAX_SEGMENT_AMOUNT {
            let Some((ts, key)) = inner.lru.pop_front() else {
                // 没有可淘汰项，只能退出（理论上不该发生）
                break;
            };
            // 只有当 entry 仍存在且 last_access 匹配时，才执行淘汰
            if let Some(entry) = inner.map.get(key.as_ref()) {
                if entry.last_access != ts {
                    continue;
                }
            } else {
                continue;
            }
            inner.map.remove(key.as_ref());
        }
    }

    fn intern_segments(&self, segments: &[String]) -> Arc<[Arc<str>]> {
        let mut guard = self
            .interner
            .write()
            .expect("depot path manager interner poisoned (write)");
        let mut out = Vec::with_capacity(segments.len());
        for s in segments {
            out.push(Self::intern_one_locked(&mut guard, s));
        }
        Self::evict_lru_locked(&mut guard);
        Arc::from(out)
    }

    fn intern_segments_str(&self, segments: &[&str]) -> Arc<[Arc<str>]> {
        let mut guard = self
            .interner
            .write()
            .expect("depot path manager interner poisoned (write)");
        let mut out = Vec::with_capacity(segments.len());
        for s in segments {
            out.push(Self::intern_one_locked(&mut guard, s));
        }
        Self::evict_lru_locked(&mut guard);
        Arc::from(out)
    }

    fn fmt(&self, path: &DepotPath, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("//")?;
        match path.kind {
            DepotPathKind::File => {
                if path.segments.is_empty() {
                    return Err(fmt::Error);
                }
                for (i, id) in path.segments.iter().enumerate() {
                    if i + 1 == path.segments.len() {
                        f.write_str(id.as_ref())?;
                    } else {
                        f.write_str(id.as_ref())?;
                        f.write_str("/")?;
                    }
                }
                Ok(())
            }
            DepotPathKind::Directory => {
                for id in path.segments.iter() {
                    f.write_str(id.as_ref())?;
                    f.write_str("/")?;
                }
                Ok(())
            }
            DepotPathKind::Wildcard => {
                for id in path.segments.iter() {
                    f.write_str(id.as_ref())?;
                    f.write_str("/")?;
                }
                f.write_str("...")
            }
        }
    }
}

/// Hive 侧对 depot path 的统一封装：
/// - 文件：`//a/b/c.txt`
/// - 目录：`//a/b/c/`（目录**必须**以 `/` 结尾）
/// - 通配：`//a/b/...`
///
/// 该类型可直接用作 `HashMap` key（`Eq + Hash`），并且能稳定地序列化为字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DepotPathKind {
    File,
    Directory,
    Wildcard,
}

/// 采用 segment interning 的 DepotPath：内部不保存完整字符串。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DepotPath {
    kind: DepotPathKind,
    // File: dirs + [file]
    // Directory: dirs
    // Wildcard: dirs（表示 `//dirs/...`）
    segments: Arc<[Arc<str>]>,
}

#[derive(Debug, Error)]
pub enum DepotPathError {
    #[error("invalid depot path: {0}")]
    Invalid(String),
}

impl DepotPath {
    /// `parse()` 的别名，提供更直观的构造入口。
    pub fn new(input: &str) -> Result<Self, DepotPathError> {
        Self::parse(input)
    }

    pub fn is_directory(&self) -> bool {
        self.kind == DepotPathKind::Directory
    }

    pub fn is_file(&self) -> bool {
        self.kind == DepotPathKind::File
    }

    pub fn is_wildcard(&self) -> bool {
        self.kind == DepotPathKind::Wildcard
    }

    /// 获取父路径（返回值一定是“目录形式”，即以 `/` 结尾）。
    ///
    /// - `//a/b/c.txt` -> `//a/b/`
    /// - `//a/b/c/` -> `//a/b/`
    /// - `//a/b/...` -> `//a/b/`
    pub fn parent(&self) -> Option<DepotPath> {
        match self.kind {
            DepotPathKind::File | DepotPathKind::Directory => {
                if self.segments.len() < 2 {
                    return None;
                }
                Some(DepotPath {
                    kind: DepotPathKind::Directory,
                    segments: Arc::from(&self.segments[..self.segments.len() - 1]),
                })
            }
            DepotPathKind::Wildcard => {
                if self.segments.is_empty() {
                    return None;
                }
                Some(DepotPath {
                    kind: DepotPathKind::Directory,
                    segments: self.segments.clone(),
                })
            }
        }
    }

    /// 解析并规范化。
    pub fn parse(input: &str) -> Result<Self, DepotPathError> {
        // hive 侧不支持 r:// 与 ~ext
        if input.starts_with("r://") {
            return Err(DepotPathError::Invalid(
                "hive DepotPath does not support 'r://'".to_string(),
            ));
        }
        if input.contains('~') {
            return Err(DepotPathError::Invalid(
                "hive DepotPath does not support '~ext' wildcard".to_string(),
            ));
        }

        // 必须是 depot path 风格
        if !input.starts_with("//") {
            return Err(DepotPathError::Invalid("must start with '//'".to_string()));
        }

        // 含 `...`：通配（hive 侧只允许 `//a/b/...`）
        if input.contains("...") {
            if input.ends_with('/') {
                return Err(DepotPathError::Invalid(
                    "wildcard path must not end with '/'".to_string(),
                ));
            }
            if !input.ends_with("...") {
                return Err(DepotPathError::Invalid(
                    "only `//.../...` (ending with `...`) wildcard is supported in hive"
                        .to_string(),
                ));
            }

            let w = CoreDepotPathWildcard::parse(input)
                .map_err(|e| DepotPathError::Invalid(e.to_string()))?;
            let CoreDepotPathWildcard::Range(range) = w else {
                return Err(DepotPathError::Invalid(
                    "only range wildcard is supported in hive".to_string(),
                ));
            };
            if !range.recursive || range.wildcard != FilenameWildcard::All {
                return Err(DepotPathError::Invalid(
                    "only `//a/b/...` wildcard is supported in hive".to_string(),
                ));
            }
            let segments = DepotPathManager::global().intern_segments(&range.dirs);
            return Ok(DepotPath {
                kind: DepotPathKind::Wildcard,
                segments,
            });
        }

        // 目录：必须以 `/` 结尾
        if input.ends_with('/') {
            let with_slash = if input.ends_with('/') {
                input.to_string()
            } else {
                format!("{input}/")
            };
            let w = CoreDepotPathWildcard::parse(&with_slash)
                .map_err(|e| DepotPathError::Invalid(e.to_string()))?;
            let CoreDepotPathWildcard::Range(range) = w else {
                return Err(DepotPathError::Invalid("invalid depot directory".to_string()));
            };
            if range.recursive || range.wildcard != FilenameWildcard::All {
                return Err(DepotPathError::Invalid("invalid depot directory".to_string()));
            }
            if range.dirs.is_empty() {
                return Err(DepotPathError::Invalid("empty depot path".to_string()));
            }

            let segments = DepotPathManager::global().intern_segments(&range.dirs);
            return Ok(DepotPath {
                kind: DepotPathKind::Directory,
                segments,
            });
        }

        // 文件：一定不以 `/` 结尾。交给 core 校验，再把 dirs + file 逐段驻留
        let f = CoreDepotPath::parse(input).map_err(|e| DepotPathError::Invalid(e.to_string()))?;
        let mut segments: Vec<&str> = Vec::with_capacity(f.dirs.len() + 1);
        for d in &f.dirs {
            segments.push(d);
        }
        segments.push(&f.file);
        let interned = DepotPathManager::global().intern_segments_str(&segments);
        Ok(DepotPath {
            kind: DepotPathKind::File,
            segments: interned,
        })
    }
}

impl fmt::Display for DepotPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        DepotPathManager::global().fmt(self, f)
    }
}

impl FromStr for DepotPath {
    type Err = DepotPathError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for DepotPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DepotPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        DepotPath::parse(&s).map_err(de::Error::custom)
    }
}

/// 给 `HashMap<DepotPath, V>` 提供基于 `&str` 的便捷查询 API。
///
/// 说明：`HashMap::contains_key` 本身要求参数类型能被 `K: Borrow<Q>` 借用。
/// 由于 `DepotPath` 内部不保留稳定的 `&str` 借用（避免额外内存占用），
/// 因此这里提供一组 helper：内部 `parse` 成临时 `DepotPath` 再查询。
pub trait DepotPathMapExt<V> {
    fn contains_key_str(&self, depot_path: &str) -> Result<bool, DepotPathError>;
    fn get_str(&self, depot_path: &str) -> Result<Option<&V>, DepotPathError>;
    fn remove_str(&mut self, depot_path: &str) -> Result<Option<V>, DepotPathError>;
    fn insert_str(&mut self, depot_path: &str, value: V) -> Result<Option<V>, DepotPathError>;
}

impl<V> DepotPathMapExt<V> for HashMap<DepotPath, V> {
    fn contains_key_str(&self, depot_path: &str) -> Result<bool, DepotPathError> {
        let key = DepotPath::parse(depot_path)?;
        Ok(self.contains_key(&key))
    }

    fn get_str(&self, depot_path: &str) -> Result<Option<&V>, DepotPathError> {
        let key = DepotPath::parse(depot_path)?;
        Ok(self.get(&key))
    }

    fn remove_str(&mut self, depot_path: &str) -> Result<Option<V>, DepotPathError> {
        let key = DepotPath::parse(depot_path)?;
        Ok(self.remove(&key))
    }

    fn insert_str(&mut self, depot_path: &str, value: V) -> Result<Option<V>, DepotPathError> {
        let key = DepotPath::parse(depot_path)?;
        Ok(self.insert(key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_file_roundtrip() {
        let p = DepotPath::parse("//a/b/c213/awd.ext").unwrap();
        assert_eq!(p.to_string(), "//a/b/c213/awd.ext");
    }

    #[test]
    fn parse_dir_normalizes_trailing_slash() {
        let d = DepotPath::parse("//as/b/sd/").unwrap();
        assert!(d.is_directory());
        assert_eq!(d.to_string(), "//as/b/sd/");

        let f = DepotPath::parse("//as/b/sd").unwrap();
        assert!(f.is_file());
        assert_eq!(f.to_string(), "//as/b/sd");
    }

    #[test]
    fn parse_wildcard_ellipsis() {
        let p = DepotPath::parse("//gae/asd/...").unwrap();
        assert_eq!(p.to_string(), "//gae/asd/...");
    }

    #[test]
    fn can_be_hashmap_key() {
        let mut m: HashMap<DepotPath, u32> = HashMap::new();
        m.insert(DepotPath::parse("//as/b/sd/").unwrap(), 42);
        assert_eq!(m.get(&DepotPath::parse("//as/b/sd/").unwrap()), Some(&42));
    }

    #[test]
    fn api_is_file_is_dir_is_wildcard() {
        let f = DepotPath::parse("//a/b/c.txt").unwrap();
        assert!(f.is_file());
        assert!(!f.is_directory());
        assert!(!f.is_wildcard());

        let d = DepotPath::parse("//a/b/c/").unwrap();
        assert!(!d.is_file());
        assert!(d.is_directory());
        assert!(!d.is_wildcard());

        let w = DepotPath::parse("//a/b/...").unwrap();
        assert!(!w.is_file());
        assert!(!w.is_directory());
        assert!(w.is_wildcard());
    }

    #[test]
    fn api_parent() {
        let f = DepotPath::parse("//a/b/c.txt").unwrap();
        assert_eq!(f.parent().unwrap().to_string(), "//a/b/");

        let d = DepotPath::parse("//a/b/c/").unwrap();
        assert_eq!(d.parent().unwrap().to_string(), "//a/b/");

        let w = DepotPath::parse("//a/b/...").unwrap();
        assert_eq!(w.parent().unwrap().to_string(), "//a/b/");
    }

    #[test]
    fn hive_rejects_regex_and_tilde_wildcards() {
        assert!(DepotPath::parse("r://.*").is_err());
        assert!(DepotPath::parse("//a/b/~.meta").is_err());
        assert!(DepotPath::parse("//a/b/...~txt.meta").is_err());
    }

    #[test]
    fn hashmap_contains_key_str() {
        let mut m: HashMap<DepotPath, u32> = HashMap::new();
        m.insert(DepotPath::parse("//as/b/sd/").unwrap(), 7);
        assert_eq!(m.contains_key_str("//as/b/sd/").unwrap(), true);
        assert_eq!(m.contains_key_str("//as/b/sd").unwrap(), false);
    }
}

