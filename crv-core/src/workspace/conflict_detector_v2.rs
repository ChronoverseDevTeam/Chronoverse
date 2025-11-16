//! # 映射冲突检测器 V2
//!
//! 该模块实现了映射冲突检测器，用于检测服务器路径到本地路径的映射关系中是否存在冲突。
//!
//! ## 算法原理
//!
//! 给定一系列从服务器路径到本地路径的映射关系，我们需要判断是否存在非法的映射，
//! 即是否有两个不同的服务器路径映射到同一个本地路径。
//!
//! ### 示例
//!
//! 考虑以下两个映射：
//! - `a/b/ -> z/x/`
//! - `a/b/c/d/ -> z/x/y/t/`
//!
//! 这会导致冲突，因为：
//! - 服务器路径 `a/b/c/d/file.txt` 根据第二条规则映射到 `z/x/y/t/file.txt`
//! - 服务器路径 `a/b/y/t/file.txt` 根据第一条规则也映射到 `z/x/y/t/file.txt`
//!
//! ## 验证算法
//!
//! 使用 `verify_mappings()` 方法：
//!
//! 1. 遍历所有映射
//! 2. 对每个映射，检查有多少个映射可以到达它的本地路径
//! 3. 如果某个本地路径可以被多个映射到达（计数 > 1），则存在冲突
//!
//! ## 路径规则
//!
//! - 以 `/` 结尾的是文件夹路径
//! - 不以 `/` 结尾的是文件路径（需要带后缀名）

use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConflictError {
    #[error("Mapping conflict detected: multiple server paths map to local path {0}")]
    PathConflict(String),
}

pub type ConflictResult<T> = Result<T, ConflictError>;

/// 文件名通配符类型（简化版本，用于冲突检测）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FilenameFilter {
    /// 匹配所有文件
    All,
    /// 匹配特定后缀名（如 "png", "txt"）
    Extension(String),
}

impl FilenameFilter {
    /// 检查两个过滤器是否兼容（可以同时满足）
    pub fn is_compatible_with(&self, other: &FilenameFilter) -> bool {
        match (self, other) {
            (FilenameFilter::All, _) | (_, FilenameFilter::All) => true,
            (FilenameFilter::Extension(ext1), FilenameFilter::Extension(ext2)) => ext1 == ext2,
        }
    }
}

/// 映射关系
#[derive(Debug, Clone)]
pub struct PathMapping {
    /// 服务器路径（depot path），如 "a/b/" 或 "a/b/file.txt"
    pub server_path: String,
    /// 本地路径，如 "local/x/" 或 "local/x/file.txt"
    pub local_path: String,
    /// 是否递归（如果为 false，则只匹配直接子文件）
    pub recursive: bool,
    /// 文件名过滤器
    pub filename_filter: FilenameFilter,
}

impl PathMapping {
    pub fn new(
        server_path: String,
        local_path: String,
        recursive: bool,
        filename_filter: FilenameFilter,
    ) -> Self {
        Self {
            server_path,
            local_path,
            recursive,
            filename_filter,
        }
    }

    /// 判断本地路径是否为文件路径（不以 '/' 结尾）
    pub fn is_file_mapping(&self) -> bool {
        !self.local_path.ends_with('/')
    }

    /// 从字符串创建映射（用于测试，默认递归且无后缀名限制）
    ///
    /// 路径规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
    pub fn from_strings(server_path: &str, local_path: &str) -> Self {
        Self::new(
            server_path.to_string(),
            local_path.to_string(),
            true,
            FilenameFilter::All,
        )
    }

    /// 从字符串创建映射，带参数（用于测试）
    ///
    /// 路径规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
    pub fn from_strings_with_params(
        server_path: &str,
        local_path: &str,
        recursive: bool,
        filename_filter: FilenameFilter,
    ) -> Self {
        Self::new(
            server_path.to_string(),
            local_path.to_string(),
            recursive,
            filename_filter,
        )
    }
}

/// 映射冲突检测器
pub struct ConflictDetector {
    mappings: Vec<PathMapping>,
}

impl ConflictDetector {
    pub fn new(mappings: Vec<PathMapping>) -> Self {
        Self { mappings }
    }

    /// 验证映射是否合法
    ///
    /// 这个方法遍历所有映射，检查是否有多个服务器路径映射到同一个本地路径，
    /// 并且它们的文件名过滤器类型相同或兼容。
    ///
    /// # 算法步骤
    ///
    /// 1. 遍历所有映射
    /// 2. 对每个映射，统计能到达它的本地路径的其他映射，按文件名过滤器分组
    /// 3. 如果某个过滤器类型的计数 > 1，或者 All 类型与其他类型共存，则存在冲突
    pub fn verify_mappings(&self) -> ConflictResult<()> {
        // 直接遍历所有映射，检查每个映射的本地路径是否有冲突
        for mapping in &self.mappings {
            let filter_counts =
                self.count_mappings_by_filter(&mapping.local_path, mapping.is_file_mapping());

            println!(
                "local_path: {}, is_file_mapping: {}, filter_counts: {:?}",
                mapping.local_path,
                mapping.is_file_mapping(),
                filter_counts
            );

            // 检查是否有冲突
            if self.has_filter_conflict(&filter_counts) {
                return Err(ConflictError::PathConflict(mapping.local_path.clone()));
            }
        }

        Ok(())
    }

    /// 统计能到达指定本地路径的映射，按文件名过滤器分组计数
    ///
    /// 返回：HashMap<FilenameFilter, usize>，键是过滤器类型，值是该类型的映射数量
    fn count_mappings_by_filter(
        &self,
        local_path: &str,
        is_file_node: bool,
    ) -> HashMap<FilenameFilter, usize> {
        let mut filter_counts = HashMap::new();

        // 对每个映射进行检查
        for (mapping_idx, mapping) in self.mappings.iter().enumerate() {
            // 检查该映射是否可以将某个服务器路径映射到 local_path
            if self.can_mapping_reach_local_path(mapping_idx, local_path, is_file_node) {
                // 按过滤器类型计数
                *filter_counts
                    .entry(mapping.filename_filter.clone())
                    .or_insert(0) += 1;
            }
        }

        filter_counts
    }

    /// 检查过滤器计数是否存在冲突
    ///
    /// 冲突条件：
    /// 1. 某个特定过滤器类型（如 Extension("png")）的计数 > 1
    /// 2. All 类型存在且计数 > 1
    /// 3. All 类型存在且与其他任何类型共存
    fn has_filter_conflict(&self, filter_counts: &HashMap<FilenameFilter, usize>) -> bool {
        // 如果没有映射能到达，肯定没有冲突
        if filter_counts.is_empty() {
            return false;
        }

        // 检查每个过滤器类型的计数
        for (filter, count) in filter_counts.iter() {
            match filter {
                FilenameFilter::All => {
                    // All 类型：如果计数 > 1，或者与其他类型共存，都是冲突
                    if *count > 1 {
                        return true;
                    }
                    if filter_counts.len() > 1 {
                        // All 与其他类型共存
                        return true;
                    }
                }
                FilenameFilter::Extension(_) => {
                    // 特定扩展名：如果计数 > 1，是冲突
                    if *count > 1 {
                        return true;
                    }
                    // 如果存在 All 类型，也是冲突
                    if filter_counts.contains_key(&FilenameFilter::All) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// 检查指定映射是否可以将某个服务器路径映射到指定的本地路径
    ///
    /// 算法：
    /// 1. 检查本地路径是否匹配（只需判断前缀）
    /// 2. 将本地路径转换为服务器路径
    /// 3. 检查映射的 server_path 是否能到达该服务器路径（判断递归、文件名过滤器）
    /// 4. 检查所有优先级更高的映射，如果它们能到达这个 local_path 且能到达该服务器路径，则当前映射被覆盖
    fn can_mapping_reach_local_path(
        &self,
        mapping_idx: usize,
        local_path: &str,
        is_file_node: bool,
    ) -> bool {
        let mapping = &self.mappings[mapping_idx];

        // 步骤1: 检查本地路径是否匹配（只需判断前缀）
        if !self.is_prefix(&mapping.local_path, local_path) {
            return false;
        }

        // 计算相对路径（从映射的本地路径到目标本地路径的差异）
        let relative_path = &local_path[mapping.local_path.len()..];

        // 步骤2: 将本地路径转换为服务器路径
        let server_path = format!("{}{}", mapping.server_path, relative_path);

        // 步骤3: 检查映射的 server_path 是否能到达该服务器路径
        // 使用从 mapping 获取的 is_file_node 信息
        if !self.can_server_path_reach(
            &mapping.server_path,
            &server_path,
            &mapping.filename_filter,
            mapping.recursive,
            is_file_node,
        ) {
            return false;
        }

        // 步骤4: 检查所有优先级更高的映射
        for higher_priority_idx in (mapping_idx + 1)..self.mappings.len() {
            let higher_mapping = &self.mappings[higher_priority_idx];

            // 检查更高优先级的映射是否能到达这个 local_path（只需判断前缀）
            if !self.is_prefix(&higher_mapping.local_path, local_path) {
                continue;
            }

            // 更高优先级的映射能到达这个 local_path
            // 现在检查它的 server_path 是否也能到达该服务器路径
            if self.can_server_path_reach(
                &higher_mapping.server_path,
                &server_path,
                &higher_mapping.filename_filter,
                higher_mapping.recursive,
                is_file_node,
            ) {
                // 如果文件名过滤器兼容，则当前映射被覆盖
                if mapping
                    .filename_filter
                    .is_compatible_with(&higher_mapping.filename_filter)
                {
                    return false;
                }
            }
        }

        true
    }

    /// 检查一个映射的 server_path 是否能到达实际的服务器路径
    ///
    /// 参数：
    /// - mapping_server_path: 映射的服务器路径
    /// - actual_server_path: 实际的服务器路径
    /// - filename_filter: 文件名过滤器
    /// - recursive: 是否递归
    /// - is_file_path: actual_server_path 是否为文件路径
    ///   规则：以 '/' 结尾的是文件夹，否则是文件（需要检查后缀名）
    ///
    /// 返回：如果能到达则返回 true
    fn can_server_path_reach(
        &self,
        mapping_server_path: &str,
        actual_server_path: &str,
        filename_filter: &FilenameFilter,
        recursive: bool,
        is_file_path: bool,
    ) -> bool {
        let mut result = true;

        // 检查前缀是否一致
        if !self.is_prefix(mapping_server_path, actual_server_path) {
            println!("not prefix");
            result = false;
        }

        // 检查递归限制
        if result {
            // 计算相对路径（从映射的 server_path 到实际的 server_path 的差异）
            let relative_path = &actual_server_path[mapping_server_path.len()..];
            if !recursive {
                if is_file_path {
                    // 文件路径（不以 '/' 结尾）：非递归时 relative_path 深度必须 <= 1
                    // 计算深度：统计 '/' 的数量
                    let depth = relative_path.matches('/').count();
                    if depth > 0 {
                        println!("not recursive and file path too deep");
                        result = false;
                    }
                } else {
                    // 文件夹路径（以 '/' 结尾）：非递归时 relative_path 深度必须 == 0
                    if !relative_path.is_empty() {
                        println!("not recursive and directory path not root");
                        result = false;
                    }
                }
            }
        }

        // 检查文件名过滤器（只有文件路径才需要检查后缀名）
        if result {
            if is_file_path {
                // 提取文件名（最后一个 '/' 之后的部分，如果没有 '/' 则是整个路径）
                let filename = actual_server_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(actual_server_path);
                if !self.filename_matches_filter(filename, filename_filter) {
                    println!("filename not matches filter");
                    result = false;
                }
            }
        }

        println!(
            "[can_server_path_reach] mapping_server_path: {}, actual_server_path: {}, result: {}, is_file_path: {}, recursive: {}",
            mapping_server_path, actual_server_path, result, is_file_path, recursive
        );
        result
    }

    /// 检查文件名是否匹配过滤器
    fn filename_matches_filter(&self, filename: &str, filter: &FilenameFilter) -> bool {
        match filter {
            FilenameFilter::All => true,
            FilenameFilter::Extension(ext) => filename.ends_with(ext),
        }
    }

    /// 检查 prefix 是否是 path 的前缀
    fn is_prefix(&self, prefix: &str, path: &str) -> bool {
        path.starts_with(prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_conflict() {
        // 测试用例：a/b/ -> z/x/, a/b/c/d/ -> z/x/y/t/
        // 规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
        // 应该检测到冲突：a/b/y/t/file.txt 和 a/b/c/d/file.txt 都映射到 z/x/y/t/file.txt
        let mappings = vec![
            PathMapping::from_strings("a/b/", "z/x/"),
            PathMapping::from_strings("a/b/c/d/", "z/x/y/t/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_err());
        println!("检测到冲突: {:?}", result.err());
    }

    #[test]
    fn test_no_conflict() {
        // 测试用例：a/b/ -> z/x/, a/b/c/d/ -> z/x/c/d/
        // 规则：以 '/' 结尾的是文件夹
        // 不应该有冲突，因为映射是一致的
        let mappings = vec![
            PathMapping::from_strings("a/b/", "z/x/"),
            PathMapping::from_strings("a/b/c/d/", "z/x/c/d/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_ok());
        println!("没有冲突");
    }

    #[test]
    fn test_multiple_mappings() {
        // 测试多个映射关系
        // 规则：以 '/' 结尾的是文件夹
        // a/b/ -> local/x/
        // a/b/c/ -> local/y/
        // a/b/c/d/ -> local/x/c/d/
        // 应该没有冲突，因为映射路径不重叠
        let mappings = vec![
            PathMapping::from_strings("a/b/", "local/x/"),
            PathMapping::from_strings("a/b/c/", "local/y/"),
            PathMapping::from_strings("a/b/c/d/", "local/x/c/d/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_ok());
        println!("多个映射：没有冲突");
    }

    #[test]
    fn test_disjoint_mappings() {
        // 测试完全不相交的映射
        // 规则：以 '/' 结尾的是文件夹
        let mappings = vec![
            PathMapping::from_strings("a/b/", "z/x/"),
            PathMapping::from_strings("c/d/", "y/w/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_ok());
        println!("没有冲突（不相交的映射）");
    }

    #[test]
    fn test_nested_conflict() {
        // 测试嵌套冲突
        // 规则：以 '/' 结尾的是文件夹
        // a/ -> local/
        // a/b/c/ -> local/x/
        // 应该检测到冲突：a/x/ 和 a/b/c/ 都映射到 local/x/
        let mappings = vec![
            PathMapping::from_strings("a/", "local/"),
            PathMapping::from_strings("a/b/c/", "local/x/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_err());
        println!("嵌套映射：检测到冲突 {:?}", result.err());
    }

    #[test]
    fn test_only_mapped_nodes() {
        // 测试只检查有映射的节点
        // 规则：以 '/' 结尾的是文件夹
        // a/b/ -> z/x/
        // a/b/c/d/ -> z/x/y/t/ (有映射)
        // 应该检测到冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "z/x/"),
            PathMapping::from_strings("a/b/c/d/", "z/x/y/t/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_err());
        println!("检测到冲突（只检查有映射的节点）: {:?}", result.err());
    }

    #[test]
    fn test_complex_scenario() {
        // 更复杂的测试场景
        // 规则：以 '/' 结尾的是文件夹
        // a/b/ -> local/x/
        // a/b/c/ -> local/y/
        // a/b/d/ -> local/z/
        // 应该没有冲突，因为映射到不同的本地路径
        let mappings = vec![
            PathMapping::from_strings("a/b/", "local/x/"),
            PathMapping::from_strings("a/b/c/", "local/y/"),
            PathMapping::from_strings("a/b/d/", "local/z/"),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        assert!(result.is_ok());
        println!("复杂场景：没有冲突");
    }

    #[test]
    fn test_non_recursive_mapping() {
        // 测试非递归映射
        // 规则：以 '/' 结尾的是文件夹
        // a/b/ -> z/x/ (非递归)
        // a/b/c/d/ -> z/x/y/t/ (递归)
        // 由于第一个映射是非递归的，它不会匹配 z/x/y/t/（深度 > 1）
        // 因此不应该有冲突
        let mappings = vec![
            PathMapping::from_strings_with_params("a/b/", "z/x/", false, FilenameFilter::All),
            PathMapping::from_strings_with_params(
                "a/b/c/d/",
                "z/x/y/t/",
                true,
                FilenameFilter::All,
            ),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("非递归映射结果: {:?}", result);
        assert!(result.is_ok());
    }

    #[test]
    fn test_non_recursive_mapping_conflict() {
        // 测试非递归映射的冲突情况
        // 规则：以 '/' 结尾的是文件夹
        // a/b/ -> z/x/ (非递归)
        // a/b/c/ -> z/x/y/ (递归)
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params("a/b/", "z/x/", false, FilenameFilter::All),
            PathMapping::from_strings_with_params("a/b/c/", "z/x/y/", true, FilenameFilter::All),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("非递归映射冲突结果: {:?}", result);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filename_filter_compatible() {
        // 测试文件名过滤器兼容的情况（应该有冲突）
        // 规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
        // a/b/ -> z/x/ (匹配 .png 文件)
        // a/b/c/d/ -> z/x/y/t/ (匹配 .png 文件)
        // 两个映射都匹配 .png 文件，应该检测到冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "z/x/",
                true,
                FilenameFilter::Extension("png".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/d/",
                "z/x/y/t/",
                true,
                FilenameFilter::Extension("png".to_string()),
            ),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("文件名过滤器兼容结果: {:?}", result);
        assert!(result.is_err());
    }

    #[test]
    fn test_filename_filter_incompatible() {
        // 测试文件名过滤器不兼容的情况（不应该有冲突）
        // 规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
        // a/b/ -> z/x/ (匹配 .png 文件)
        // a/b/c/d/ -> z/x/y/t/ (匹配 .txt 文件)
        // 两个映射匹配不同的文件类型，不应该有冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "z/x/",
                true,
                FilenameFilter::Extension("png".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/d/",
                "z/x/y/t/",
                true,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("文件名过滤器不兼容结果: {:?}", result);
        assert!(result.is_ok());
    }

    #[test]
    fn test_filename_filter_all_vs_extension() {
        // 测试 All 过滤器与特定后缀名过滤器的情况（应该有冲突）
        // 规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
        // a/b/ -> z/x/ (匹配所有文件)
        // a/b/c/d/ -> z/x/y/t/ (匹配 .png 文件)
        // All 与任何过滤器都兼容，应该检测到冲突
        let mappings = vec![
            PathMapping::from_strings_with_params("a/b/", "z/x/", true, FilenameFilter::All),
            PathMapping::from_strings_with_params(
                "a/b/c/d/",
                "z/x/y/t/",
                true,
                FilenameFilter::Extension("png".to_string()),
            ),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("All vs Extension 结果: {:?}", result);
        assert!(result.is_err());
    }

    #[test]
    fn test_complex_recursive_and_filter() {
        // 测试复杂场景：递归 + 文件过滤器
        // 规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
        // a/b/ -> z/x/ (非递归, 匹配 .png 文件)
        // a/b/c/ -> z/x/y/ (递归, 匹配 .txt 文件)
        // a/b/d/ -> z/x/z/ (递归, 匹配 .png 文件)
        // 第一个映射是非递归的，只匹配 z/x/ 下一层的 .png 文件
        // 第二个映射匹配 .txt 文件，与第一个不冲突
        // 第三个映射匹配 .png 文件，但 z/x/z/ 深度为 2，第一个映射不会匹配
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "z/x/",
                false,
                FilenameFilter::Extension("png".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "z/x/y/",
                true,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/d/",
                "z/x/z/",
                true,
                FilenameFilter::Extension("png".to_string()),
            ),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("复杂递归和过滤器结果: {:?}", result);
        assert!(result.is_ok());
    }

    #[test]
    fn test_non_recursive_with_direct_child() {
        // 测试非递归映射与直接子节点的冲突
        // 规则：以 '/' 结尾的是文件夹，否则是文件（需要带后缀名）
        // a/b/ -> z/x/ (非递归, All)
        // a/b/c/ -> z/x/y/ (递归, All)
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params("a/b/", "z/x/", false, FilenameFilter::All),
            PathMapping::from_strings_with_params("a/b/c/", "z/x/y/", true, FilenameFilter::All),
        ];

        let detector = ConflictDetector::new(mappings);
        let result = detector.verify_mappings();

        println!("非递归与直接子节点结果: {:?}", result);
        assert!(result.is_ok());
    }

    // ============ 从 entity.rs 转换的测试用例 ============

    #[test]
    fn test_entity_case_1() {
        // 原始: //a/b/c/... //workspace/
        //       //a/b/c/d/... //workspace/d/
        let mappings = vec![
            PathMapping::from_strings("a/b/c/", "workspace/"),
            PathMapping::from_strings("a/b/c/d/", "workspace/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_case_2() {
        // 原始: //a/b/...~a //workspace/a/b/
        //       //a/b/...~b //workspace/a/b/
        //       //a/b/txt.a //workspace/a/b/txt.a
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                true,
                FilenameFilter::Extension("a".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                true,
                FilenameFilter::Extension("b".to_string()),
            ),
            PathMapping::new(
                "a/b/txt.a".to_string(),
                "workspace/a/b/txt.a".to_string(),
                false,
                FilenameFilter::Extension("a".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_case_3() {
        // 原始: //a/b/txt.a //workspace/a/b/
        //       //a/b/txt.a //workspace/a/b/txt.a
        let mappings = vec![
            PathMapping::new(
                "a/b/txt.a".to_string(),
                "workspace/a/b/".to_string(),
                false,
                FilenameFilter::Extension("a".to_string()),
            ),
            PathMapping::new(
                "a/b/txt.a".to_string(),
                "workspace/a/b/txt.a".to_string(),
                false,
                FilenameFilter::Extension("a".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_case_4() {
        // 原始: //a/b/~a //workspace/a/b/
        //       //a/b/~b //workspace/a/b/
        //       //a/b/txt.a //workspace/a/b/txt.a
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::Extension("a".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::Extension("b".to_string()),
            ),
            PathMapping::new(
                "a/b/txt.a".to_string(),
                "workspace/a/b/txt.a".to_string(),
                false,
                FilenameFilter::Extension("a".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_1() {
        // 原始: //a/b/...     //workspace/a/b/
        //       //a/b/c/...   //workspace/a/b/c/d/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::from_strings("a/b/c/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_2() {
        // 原始: //a/b/...     //workspace/a/b/
        //       //a/b/c/e/... //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::from_strings("a/b/c/e/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_3() {
        // 原始: //a/b/...     //workspace/a/b/
        //       //a/b/c/e/    //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::from_strings_with_params(
                "a/b/c/e/",
                "workspace/a/b/c/d/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_4() {
        // 原始: //a/b/        //workspace/a/b/
        //       //a/b/c/e/... //workspace/a/b/c/d/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::from_strings("a/b/c/e/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_5() {
        // 原始: //a/b/c/d/... //workspace/a/b/
        //       //a/b/c/...   //workspace/a/b/c/d/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/c/d/", "workspace/a/b/"),
            PathMapping::from_strings("a/b/c/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_6() {
        // 原始: //a/b/c/d/... //workspace/a/b/
        //       //a/b/c/      //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/c/d/", "workspace/a/b/"),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/c/d/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_7() {
        // 原始: //a/b/c/d/    //workspace/a/b/
        //       //a/b/c/      //workspace/a/b/c/d/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/c/d/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/c/d/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_8() {
        // 原始: //a/b/...     //workspace/a/b/
        //       //a/b/c/d/e/... //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::from_strings("a/b/c/d/e/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_9() {
        // 原始: //a/b/...    //workspace/a/b/
        //       //a/b/c/d/e/ //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::from_strings_with_params(
                "a/b/c/d/e/",
                "workspace/a/b/c/d/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_10() {
        // 原始: //a/b/       //workspace/a/b/
        //       //a/b/c/d/e/... //workspace/a/b/c/d/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::from_strings("a/b/c/d/e/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_11() {
        // 原始: //a/b/d/...  //workspace/a/b/
        //       //a/b/c/...  //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/d/", "workspace/a/b/"),
            PathMapping::from_strings("a/b/c/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_12() {
        // 原始: //a/b/d/...  //workspace/a/b/
        //       //a/b/c/     //workspace/a/b/c/d/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/d/", "workspace/a/b/"),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/c/d/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_13() {
        // 原始: //a/b/d/     //workspace/a/b/
        //       //a/b/c/...  //workspace/a/b/c/d/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/d/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::from_strings("a/b/c/", "workspace/a/b/c/d/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_14() {
        // 原始: //a/b/d/...  //workspace/a/b/c/d/
        //       //a/b/...    //workspace/a/b/
        // 无冲突（优先级）
        let mappings = vec![
            PathMapping::from_strings("a/b/d/", "workspace/a/b/c/d/"),
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_15() {
        // 原始: //a/b/c/...  //workspace/a/b/c/d/
        //       //a/b/d/...  //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/c/", "workspace/a/b/c/d/"),
            PathMapping::from_strings("a/b/d/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_16() {
        // 原始: //a/b/c/...  //workspace/a/b/c/d/
        //       //a/b/d/     //workspace/a/b/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/c/", "workspace/a/b/c/d/"),
            PathMapping::from_strings_with_params(
                "a/b/d/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_17() {
        // 原始: //a/b/...   //workspace/a/b/c/d/
        //       //a/b/d/... //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/c/d/"),
            PathMapping::from_strings("a/b/d/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_18() {
        // 原始: //a/b/...   //workspace/a/b/c/d/
        //       //a/b/d/    //workspace/a/b/
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/c/d/"),
            PathMapping::from_strings_with_params(
                "a/b/d/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_19() {
        // 原始: //a/b/c/   //workspace/a/b/
        //       //a/b/     //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_20() {
        // 原始: //a/b/c/... //workspace/a/b/
        //       //a/b/...  //workspace/a/b/
        // 无冲突（优先级）
        let mappings = vec![
            PathMapping::from_strings("a/b/c/", "workspace/a/b/"),
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_conflict_21() {
        // 原始: //a/b/c/   //workspace/a/b/
        //       //a/b/d/   //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::from_strings_with_params(
                "a/b/d/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_conflict_22() {
        // 原始: //a/b/...  //workspace/a/b/
        //       //a/b/d/   //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::from_strings_with_params(
                "a/b/d/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_1() {
        // 原始: //a/b/c/... //workspace/a/b/
        //       //a/b/d/old_name.txt //workspace/a/b/1/new_name.txt
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/c/", "workspace/a/b/"),
            PathMapping::new(
                "a/b/d/old_name.txt".to_string(),
                "workspace/a/b/1/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_2() {
        // 原始: //a/b/c/   //workspace/a/b/
        //       //a/b/d/old_name.txt //workspace/a/b/1/new_name.txt
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::new(
                "a/b/d/old_name.txt".to_string(),
                "workspace/a/b/1/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_3() {
        // 原始: //a/b/c/ //workspace/a/b/
        //       //a/b/d/old_name.txt //workspace/a/b/new_name.txt
        // 冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::new(
                "a/b/d/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_4() {
        // 原始: //a/b/ //workspace/a/b/
        //       //a/b/file.txt //workspace/a/b/file.txt
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_5() {
        // 原始: //a/b/... //workspace/a/b/
        //       //a/b/file.txt //workspace/a/b/file.txt
        // 无冲突（优先级）
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_6() {
        // 原始: //a/b/ //workspace/a/b/
        //       //a/b/old_name.txt //workspace/a/b/new_name.txt
        // 冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::new(
                "a/b/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_7() {
        // 原始: //a/b/... //workspace/a/b/
        //       //a/b/c/old_name.txt //workspace/a/b/new_name.txt
        // 冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
            PathMapping::new(
                "a/b/c/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_8() {
        // 原始: //a/b/ //workspace/a/b/
        //       //a/b/old_name.txt //workspace/a/b/c/new_name.txt
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
            PathMapping::new(
                "a/b/old_name.txt".to_string(),
                "workspace/a/b/c/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_9() {
        // 原始: //a/b/d/... //workspace/a/b/c/
        //       //a/b/c/old_name.txt //workspace/a/b/new_name.txt
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/d/", "workspace/a/b/c/"),
            PathMapping::new(
                "a/b/c/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_10() {
        // 原始: //a/b/... //workspace/a/b/c/
        //       //a/b/c/old_name.txt //workspace/a/b/new_name.txt
        // 无冲突
        let mappings = vec![
            PathMapping::from_strings("a/b/", "workspace/a/b/c/"),
            PathMapping::new(
                "a/b/c/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_11() {
        // 原始: //a/b/old_name.txt //workspace/a/b/new_name.txt
        //       //a/b/c/ //workspace/a/b/c/
        // 无冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/c/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_12() {
        // 原始: //a/b/old_name.txt //workspace/a/b/new_name.txt
        //       //a/b/... //workspace/a/b/
        // 无冲突（优先级）
        let mappings = vec![
            PathMapping::new(
                "a/b/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings("a/b/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_13() {
        // 原始: //a/b/old_name.txt //workspace/a/b/new_name.txt
        //       //a/b/ //workspace/a/b/
        // 无冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_14() {
        // 原始: //a/b/new_name.txt //workspace/a/b/new_name.txt
        //       //a/b/ //workspace/a/b/
        // 无冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/new_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_15() {
        // 原始: //a/b/c/old_name.txt //workspace/a/b/new_name.txt
        //       //a/b/ //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/c/old_name.txt".to_string(),
                "workspace/a/b/new_name.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_16() {
        // 原始: //a/b/file.txt //workspace/a/b/file.txt
        //       //a/b/ //workspace/a/b/c/
        // 无冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/c/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_17() {
        // 原始: //a/b/c/file.txt //workspace/a/b/file.txt
        //       //a/b/ //workspace/a/b/c/
        // 无冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/c/file.txt".to_string(),
                "workspace/a/b/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/",
                "workspace/a/b/c/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_18() {
        // 原始: //a/b/file.txt //workspace/a/b/c/file.txt
        //       //a/b/c/ //workspace/a/b/
        // 无冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/c/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_ok());
    }

    #[test]
    fn test_entity_file_mapping_19() {
        // 原始: //a/b/file.txt //workspace/a/b/c/file.txt
        //       //a/b/c/... //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/c/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings("a/b/c/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_20() {
        // 原始: //a/b/file.txt //workspace/a/b/file.txt
        //       //a/b/c/ //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings_with_params(
                "a/b/c/",
                "workspace/a/b/",
                false,
                FilenameFilter::All,
            ),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }

    #[test]
    fn test_entity_file_mapping_21() {
        // 原始: //a/b/file.txt //workspace/a/b/file.txt
        //       //a/b/c/... //workspace/a/b/
        // 冲突
        let mappings = vec![
            PathMapping::new(
                "a/b/file.txt".to_string(),
                "workspace/a/b/file.txt".to_string(),
                false,
                FilenameFilter::Extension("txt".to_string()),
            ),
            PathMapping::from_strings("a/b/c/", "workspace/a/b/"),
        ];
        let detector = ConflictDetector::new(mappings);
        assert!(detector.verify_mappings().is_err());
    }
}
