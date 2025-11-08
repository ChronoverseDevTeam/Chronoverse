use crate::path::basic::*;
use crate::workspace::entity::*;
use lru::LruCache;
use regex::Regex;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// 路径引擎核心结构
pub struct PathEngine {
    workspace: Arc<WorkspaceConfig>,
    // 缓存编译的正则表达式
    cache: PathCache,
}

/// 路径缓存（用于性能优化）
pub struct PathCache {
    regex_cache: Arc<Mutex<LruCache<String, Arc<Regex>>>>,
    max_capacity: usize,
}

impl PathCache {
    const DEFAULT_CACHE_CAPACITY: usize = 100;

    pub fn new(max_capacity: usize) -> Self {
        Self {
            regex_cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(max_capacity).unwrap(),
            ))),
            max_capacity,
        }
    }

    pub fn get_or_compile(&self, pattern: &str) -> PathResult<Arc<regex::Regex>> {
        let mut cache = self.regex_cache.lock().unwrap();

        // 尝试从缓存获取
        if let Some(regex) = cache.get(pattern) {
            return Ok(Arc::clone(regex));
        }

        // 缓存未命中，编译新的 regex
        drop(cache); // 释放锁，避免长时间持有

        let regex = Arc::new(regex::Regex::new(pattern)?);

        // 重新获取锁并插入
        let mut cache = self.regex_cache.lock().unwrap();
        cache.put(pattern.to_string(), Arc::clone(&regex));

        Ok(regex)
    }

    pub fn clear(&self) {
        self.regex_cache.lock().unwrap().clear();
    }
}

impl Default for PathCache {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CACHE_CAPACITY)
    }
}

impl PathEngine {
    /// 创建新的路径引擎
    pub fn new(workspace: WorkspaceConfig) -> Self {
        Self {
            workspace: Arc::new(workspace),
            cache: PathCache::default(),
        }
    }

    /// 将一个本地路径转化为 depot 路径，如果本地路径不在工作区内，返回 None
    pub fn mapping_local_path(&self, local_path: &LocalPath) -> Option<DepotPath> {
        // 反向遍历所有 mapping
        for mapping in self.workspace.mappings.iter().rev() {
            match mapping {
                WorkspaceMapping::Include(include_mapping) => match include_mapping {
                    IncludeMapping::File(file_mapping) => {
                        if &file_mapping.local_file == local_path {
                            let mapping_back = self.mapping_depot_path(&file_mapping.depot_file);
                            if mapping_back.is_some() && &mapping_back.unwrap() == local_path {
                                // 如果还能映射回来，说明没有被排除掉
                                return Some(file_mapping.depot_file.clone());
                            } else {
                                // 否则，说明被排除了。
                                // 又因不考虑排除的时候，depot path 与 local path 不存在多对一关系，
                                // 所以不用再向上遍历了
                                return None;
                            }
                        }
                    }
                    IncludeMapping::Folder(folder_mapping) => {
                        if !folder_mapping
                            .depot_folder
                            .wildcard
                            .check_match(&local_path.file)
                        {
                            continue;
                        }
                        let diff = folder_mapping
                            .local_folder
                            .match_and_get_diff(&local_path.dirs);
                        if diff.is_none() {
                            continue;
                        }
                        let diff = diff.unwrap();
                        let mut depot_dir_candidate = Vec::new();
                        depot_dir_candidate.extend_from_slice(&folder_mapping.depot_folder.dirs);
                        depot_dir_candidate.extend_from_slice(diff);
                        let depot_path_candidate = DepotPath {
                            dirs: depot_dir_candidate,
                            file: local_path.file.clone(),
                        };
                        let mapping_back = self.mapping_depot_path(&depot_path_candidate);
                        if mapping_back.is_some() && &mapping_back.unwrap() == local_path {
                            return Some(depot_path_candidate);
                        } else {
                            // 同理，不用再向上遍历了
                            return None;
                        }
                    }
                },
                WorkspaceMapping::Exclude(_) => continue,
            }
        }
        None
    }

    /// 将一个 depot 路径映射为该工作区下的本地路径，如 depot 路径不在工作区内，返回 None
    pub fn mapping_depot_path(&self, depot_path: &DepotPath) -> Option<LocalPath> {
        // 反向遍历所有 mapping
        for mapping in self.workspace.mappings.iter().rev() {
            match mapping {
                WorkspaceMapping::Include(include_mapping) => match include_mapping {
                    IncludeMapping::File(file_mapping) => {
                        if depot_path == &file_mapping.depot_file {
                            return Some(file_mapping.local_file.clone());
                        }
                    }
                    IncludeMapping::Folder(folder_mapping) => {
                        let diff = folder_mapping.depot_folder.match_and_get_diff(depot_path);
                        if diff.is_none() {
                            continue;
                        }
                        let diff = diff.unwrap();
                        let mut local_dir = vec![];
                        local_dir.extend_from_slice(&folder_mapping.local_folder.0);
                        local_dir.extend_from_slice(diff);
                        let local_file = LocalPath {
                            dirs: LocalDir(local_dir),
                            file: depot_path.file.clone(),
                        };
                        return Some(local_file);
                    }
                },
                WorkspaceMapping::Exclude(ExcludeMapping(excluded_depot_path_wildcard)) => {
                    match excluded_depot_path_wildcard {
                        DepotPathWildcard::Range(range_depot_wildcard) => {
                            if range_depot_wildcard
                                .match_and_get_diff(depot_path)
                                .is_some()
                            {
                                return None;
                            }
                        }
                        DepotPathWildcard::Regex(regex_depot_wildcard) => {
                            let pattern = self.cache.get_or_compile(&regex_depot_wildcard.pattern);
                            // 正常来讲，WorkspaceConfig 应当是经过校验的，不会存在不能编译的正则
                            if pattern.is_err() {
                                continue;
                            }
                            let pattern = pattern.unwrap();
                            if pattern.is_match(&depot_path.to_string()) {
                                return None;
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapping_depot() {
        let config = WorkspaceConfig::from_specification(
            "/home/youzheyin/Tree/Chronoverse/crv-cli",
            r#"
            //crv/cli/src/... /home/youzheyin/Tree/Chronoverse/crv-cli/src
            -//crv/cli/src/~png
            -//crv/cli/somefile
            //crv/assets/... /home/youzheyin/Tree/Chronoverse/crv-cli/assets
            -//crv/assets/...~jpg
            //crv/assets/special-image/... /home/youzheyin/Tree/Chronoverse/crv-cli/assets/special-image
            "#,
        )
        .unwrap();

        let engine = PathEngine::new(config);

        let depot_file = DepotPath::parse("//crv/cli/src/main.rs").unwrap();
        let result = engine.mapping_depot_path(&depot_file).unwrap();
        assert_eq!(
            result,
            LocalPath::parse("/home/youzheyin/Tree/Chronoverse/crv-cli/src/main.rs").unwrap(),
        );
        let depot_file = DepotPath::parse("//crv/cli/src/template/logo.png").unwrap();
        let result = engine.mapping_depot_path(&depot_file).unwrap();
        assert_eq!(
            result,
            LocalPath::parse("/home/youzheyin/Tree/Chronoverse/crv-cli/src/template/logo.png")
                .unwrap(),
        );
        let depot_file = DepotPath::parse("//crv/assets/special-image/cat.jpg").unwrap();
        let result = engine.mapping_depot_path(&depot_file).unwrap();
        assert_eq!(
            result,
            LocalPath::parse(
                "/home/youzheyin/Tree/Chronoverse/crv-cli/assets/special-image/cat.jpg"
            )
            .unwrap(),
        );
        let depot_file = DepotPath::parse("//crv/assets/normal.jpg").unwrap();
        assert!(engine.mapping_depot_path(&depot_file).is_none());
        let depot_file = DepotPath::parse("//crv/assets/a/b/normal.jpg").unwrap();
        assert!(engine.mapping_depot_path(&depot_file).is_none());
        let depot_file = DepotPath::parse("//crv/cli/src/temp.png").unwrap();
        assert!(engine.mapping_depot_path(&depot_file).is_none());
        let depot_file = DepotPath::parse("//crv/cli/cargo.toml").unwrap();
        assert!(engine.mapping_depot_path(&depot_file).is_none());
    }

    #[test]
    fn test_mapping_local() {
        let config = WorkspaceConfig::from_specification(
            "/root/test",
            r#"
            //a/...~a /root/test/a/
            //b/...~b /root/test/a/b
            -r://.*\.b
            //a/b/c/d.txt /root/test/a/d.ini
            -//a/b/c/...~txt
            //a/b/c/d/e.txt /root/test/a/e1.ini
            //a/b/c/d/e.txt /root/test/a/e.ini
            "#,
        )
        .unwrap();

        let engine = PathEngine::new(config);
        let local_path = LocalPath::parse("/root/test/a/b/z.a").unwrap();
        assert_eq!(
            engine.mapping_local_path(&local_path).unwrap(),
            DepotPath::parse("//a/b/z.a").unwrap()
        );
        let local_path = LocalPath::parse("/root/test/a/b/z.b").unwrap();
        assert!(engine.mapping_local_path(&local_path).is_none());
        let local_path = LocalPath::parse("/root/test/a/d.ini").unwrap();
        assert!(engine.mapping_local_path(&local_path).is_none());
        let local_path = LocalPath::parse("/root/test/a/e.ini").unwrap();
        assert_eq!(
            engine.mapping_local_path(&local_path).unwrap(),
            DepotPath::parse("//a/b/c/d/e.txt").unwrap()
        );
        let local_path = LocalPath::parse("/root/test/a/e1.ini").unwrap();
        assert!(engine.mapping_local_path(&local_path).is_none());
        let local_path = LocalPath::parse("/root/test/a/b/z-a").unwrap();
        assert!(engine.mapping_local_path(&local_path).is_none());
    }
}
