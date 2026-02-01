use crate::parsers;
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 求两个切片的公共前缀的结束索引，如果返回 0 则代表没有公共前缀
fn common_prefix_end_index(slice_a: &[String], slice_b: &[String]) -> usize {
    for (i, (a, b)) in slice_a.iter().zip(slice_b.iter()).enumerate() {
        if a != b {
            return i;
        }
    }
    slice_a.len().min(slice_b.len())
}

#[derive(Error, Debug)]
pub enum PathError {
    #[error("Syntax error: {0}")]
    SyntaxError(String),

    #[error("Regex compilation error: {0}")]
    RegexError(#[from] regex::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type PathResult<T> = Result<T, PathError>;

/// 版本描述符
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionDescriptor {
    /// 版本ID: #33
    RevisionId(u64),
    /// Changelist编号: @3588021
    ChangelistNumber(u64),
}

/// 文件名通配符类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub enum FilenameWildcard {
    /// 确切的文件名
    Exact(String),
    /// 后缀名通配: ~png, ~obj.meta, 将 `~` 替换为 `.`
    Extension(String),
    /// 匹配所有文件
    All,
}

impl FilenameWildcard {
    fn to_custom_string(&self) -> String {
        match self {
            FilenameWildcard::Exact(s) => s.clone(),
            FilenameWildcard::Extension(s) => format!("~{}", &s[1..]),
            FilenameWildcard::All => String::new(),
        }
    }
    /// 检查一个文件名是否匹配该 wildcard
    pub fn check_match(&self, filename: &str) -> bool {
        match self {
            FilenameWildcard::Exact(exact) => filename == exact,
            FilenameWildcard::Extension(extension) => filename.ends_with(extension),
            FilenameWildcard::All => true,
        }
    }
}

/// Workspace Path (具体的文件)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct WorkspacePath {
    pub workspace_name: String,
    pub dirs: Vec<String>,
    pub file: String,
}

impl WorkspacePath {
    pub fn parse(path: &str) -> PathResult<Self> {
        parsers::path::workspace_path(path)
    }

    pub fn to_custom_string(&self) -> String {
        format!(
            "//{}/{}{}",
            self.workspace_name,
            self.dirs
                .iter()
                .map(|dir| format!("{}/", dir))
                .collect::<String>(),
            self.file
        )
    }

    pub fn to_local_path_uncheck(&self, root_dir: &LocalDir) -> LocalPath {
        let mut dirs = root_dir.0.clone();
        dirs.extend_from_slice(&self.dirs);

        LocalPath {
            dirs: LocalDir(dirs),
            file: self.file.clone(),
        }
    }
}

/// Workspace 目录
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDir {
    pub workspace_name: String,
    pub dirs: Vec<String>,
}

impl WorkspaceDir {
    pub fn parse(path: &str) -> PathResult<Self> {
        parsers::path::workspace_dir(path)
    }

    pub fn to_custom_string(&self) -> String {
        let dir_string = self
            .dirs
            .iter()
            .map(|dir| format!("{}/", dir))
            .collect::<String>();
        format!("//{}/{}", self.workspace_name, dir_string)
    }

    pub fn to_local_dir_uncheck(&self, root_dir: &LocalDir) -> LocalDir {
        let mut dirs = root_dir.0.clone();
        dirs.extend_from_slice(&self.dirs);
        LocalDir(dirs)
    }
}

/// Depot Path (具体的文件)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct DepotPath {
    pub dirs: Vec<String>,
    pub file: String,
}

impl DepotPath {
    pub fn to_custom_string(&self) -> String {
        format!(
            "//{}{}",
            self.dirs
                .iter()
                .map(|dir| format!("{}/", dir))
                .collect::<String>(),
            self.file
        )
    }

    pub fn parse(path: &str) -> PathResult<Self> {
        parsers::path::depot_path(path)
    }
}

/// 通配 Depot Path
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum DepotPathWildcard {
    /// 范围索引路径: //dir/...~png、//dir/~jpg.meta、//dir/...
    Range(RangeDepotWildcard),
    /// 正则表达式路径: r://project/.*\.rs
    Regex(RegexDepotWildcard),
}

impl DepotPathWildcard {
    pub fn to_custom_string(&self) -> String {
        match self {
            DepotPathWildcard::Range(range_depot_wildcard) => {
                let filename_wildcard_string = range_depot_wildcard.wildcard.to_custom_string();

                format!(
                    "//{}{}{}",
                    range_depot_wildcard
                        .dirs
                        .iter()
                        .map(|dir| format!("{}/", dir))
                        .collect::<String>(),
                    if range_depot_wildcard.recursive {
                        "..."
                    } else {
                        ""
                    },
                    filename_wildcard_string,
                )
            }
            DepotPathWildcard::Regex(regex_depot_wildcard) => {
                format!("r://{}", regex_depot_wildcard.pattern)
            }
        }
    }

    pub fn parse(wildcard: &str) -> PathResult<Self> {
        parsers::path::depot_path_wildcard(wildcard)
    }
}

/// 范围索引 Depot Path
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct RangeDepotWildcard {
    /// 目录部分
    pub dirs: Vec<String>,
    /// 是否包含递归通配符 ...
    pub recursive: bool,
    /// 文件通配符
    pub wildcard: FilenameWildcard,
}

impl RangeDepotWildcard {
    /// 判断一个 depot path 是否被该 wildcard 匹配，如果匹配则返回 depot path 独有的目录部分，否则返回 None
    pub fn match_and_get_diff<'a>(&self, depot_path: &'a DepotPath) -> Option<&'a [String]> {
        let common_prefix_end = common_prefix_end_index(&self.dirs, &depot_path.dirs);
        if common_prefix_end != self.dirs.len() {
            return None;
        }
        if !self.recursive && common_prefix_end != depot_path.dirs.len() {
            return None;
        }
        if !self.wildcard.check_match(&depot_path.file) {
            return None;
        }
        Some(&depot_path.dirs[common_prefix_end..])
    }
}

/// 正则 Depot Path
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct RegexDepotWildcard {
    /// 原始正则表达式字符串
    pub pattern: String,
}

/// 本地目录路径（规范化后的绝对路径，精确到目录）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct LocalDir(pub Vec<String>);

impl LocalDir {
    pub fn parse(path: &str) -> PathResult<Self> {
        parsers::path::local_dir(path)
    }

    /// 转化为使用 `/` 分割文件夹的路径
    pub fn to_unix_path_string(&self) -> String {
        let dir_string = self
            .0
            .iter()
            .map(|dir| format!("{}/", dir))
            .collect::<String>();
        format!("/{}", dir_string)
    }

    /// 转化为当前平台本地风格的路径字符串（规范化的绝对路径）
    pub fn to_local_path_string(&self) -> String {
        #[cfg(windows)]
        {
            // Windows 逻辑：
            // 假设 dirs[0] 是盘符 (如 "C:")
            let (drive, sub_dirs) = self.0.split_first().unwrap();
            // 如果路径是 C: 和 file.txt -> C:\file.txt
            // 如果路径是 C:, Users 和 file.txt -> C:\Users\file.txt
            let mut full_path = drive.clone();
            if !full_path.ends_with(":") {
                full_path.push(':');
            }

            // 确保盘符后有分隔符
            full_path.push('\\');

            for dir in sub_dirs {
                full_path.push_str(dir);
                full_path.push('\\');
            }
            full_path
        }

        #[cfg(not(windows))]
        {
            // Unix 逻辑：
            // 始终以 / 开头，连接所有目录
            if self.0.is_empty() {
                format!("/")
            } else {
                let dir_part = self.0.join("/");
                format!("/{}/", dir_part)
            }
        }
    }

    /// 判断一个 local dir 是否在该 dir 下，如果在则返回 local dir 独有的目录部分，否则返回 None
    pub fn match_and_get_diff<'a>(&self, other: &'a LocalDir) -> Option<&'a [String]> {
        let common_prefix_end = common_prefix_end_index(&self.0, &other.0);
        if common_prefix_end != self.0.len() {
            return None;
        }
        Some(&other.0[common_prefix_end..])
    }
}

/// 本地路径（规范化后的绝对路径，精确到文件）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Encode, Decode)]
pub struct LocalPath {
    pub dirs: LocalDir,
    pub file: String,
}

impl LocalPath {
    /// 平台无关的绝对路径解析器，
    /// 能够解析 Windows 本地路径以及类 Unix 路径。
    ///
    /// Windows 本地路径风格为 `<盘符>:` 开头的，使用 `/` 或 `\` 进行分割的路径，
    /// 在解析后 `self.dirs` 的第一个元素即为盘符，保持原有的大小写。
    pub fn parse(path: &str) -> PathResult<Self> {
        parsers::path::local_path(path)
    }

    /// 转化为使用 `/` 分割文件夹的路径
    pub fn to_unix_path_string(&self) -> String {
        let dir_string = self
            .dirs
            .0
            .iter()
            .map(|dir| format!("{}/", dir))
            .collect::<String>();
        format!("/{}{}", dir_string, self.file)
    }

    /// 转化为当前平台本地风格的路径字符串
    pub fn to_local_path_string(&self) -> String {
        #[cfg(windows)]
        {
            // Windows 逻辑：
            // 假设 dirs[0] 是盘符 (如 "C:")
            let (drive, sub_dirs) = self.dirs.0.split_first().unwrap();
            // 如果路径是 C: 和 file.txt -> C:\file.txt
            // 如果路径是 C:, Users 和 file.txt -> C:\Users\file.txt
            let mut full_path = drive.clone();
            if !full_path.ends_with(":") {
                full_path.push(':');
            }

            // 确保盘符后有分隔符
            full_path.push('\\');

            for dir in sub_dirs {
                full_path.push_str(dir);
                full_path.push('\\');
            }
            full_path.push_str(&self.file);

            println!("full_path: {:?}", full_path);
            println!("self.file: {:?}", self.file);
            println!("self.dirs: {:?}", self.dirs);
            full_path
        }

        #[cfg(not(windows))]
        {
            // Unix 逻辑：
            // 始终以 / 开头，连接所有目录和文件
            if self.dirs.0.is_empty() {
                format!("/{}", self.file)
            } else {
                let dir_part = self.dirs.0.join("/");
                format!("/{}/{}", dir_part, self.file)
            }
        }
    }
}

/// 本地路径通配符
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalPathWildcard {
    /// 目录部分
    pub dirs: LocalDir,
    /// 是否包含递归通配符 ...
    pub recursive: bool,
    /// 文件通配符
    pub wildcard: FilenameWildcard,
}

impl LocalPathWildcard {
    pub fn to_custom_string(&self) -> String {
        let filename_wildcard_string = self.wildcard.to_custom_string();

        format!(
            "/{}{}{}",
            self.dirs
                .0
                .iter()
                .map(|dir| format!("{}/", dir))
                .collect::<String>(),
            if self.recursive { "..." } else { "" },
            filename_wildcard_string,
        )
    }

    pub fn parse(wildcard: &str) -> PathResult<Self> {
        parsers::path::local_path_wildcard(wildcard)
    }

    /// 判断一个 local path 是否在该 dir 下，如果在则返回 local path 独有的目录部分，否则返回 None
    pub fn match_and_get_diff<'a>(&self, local_path: &'a LocalPath) -> Option<&'a [String]> {
        let local_dir_diff = self.dirs.match_and_get_diff(&local_path.dirs)?;
        if !self.recursive && local_dir_diff.len() != 0 {
            return None;
        }
        if !self.wildcard.check_match(&local_path.file) {
            return None;
        }
        Some(local_dir_diff)
    }
}

#[cfg(test)]
mod test_depot_path {
    use super::*;
    #[test]
    fn test_depot_path_parse() {
        // 1. 正确解析
        let path = "//crv/cli/src/build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "//crv/cli/src/build.rs");

        let path = "//crv/cli/src/新建文本文档.txt";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(
            depot_path.to_custom_string(),
            "//crv/cli/src/新建文本文档.txt"
        );

        // 2. 语法错误
        let path = "///crv/cli/src/build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "//crv//cli/src/build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "/crv/cli/src/build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "//crv/cli/src/";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 3. 非法字符 `...`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/src.../build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "//crv/cli/src/...";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 文件名以空格结尾
        let path = "//crv/cli/src/新建文本文档.txt ";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 4. 非法字符 `~`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/src/~build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 5. 其他非法字符 `"*\|/<>?:`
        let path = "//crv/cli/s?rc/build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);
    }

    #[test]
    fn test_range_depot_wildcard_parse() {
        // 1. 正确解析
        let path = "//crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "//crv/cli/src/build.rs");

        let path = "//crv/cli/src/";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "//crv/cli/src/");

        let path = "//crv/cli/src/...";
        let depot_path = DepotPathWildcard::parse(path);
        println!("{:?}", depot_path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "//crv/cli/src/...");

        let path = "//crv/cli/src/...~txt.meta";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "//crv/cli/src/...~txt.meta");

        let path = "//crv/cli/src/~.meta";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "//crv/cli/src/~.meta");

        // 2. 语法错误
        let path = "///crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "//crv//cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "/crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 文件名以空格结尾
        let path = "//crv/cli/src/新建文本文档.txt ";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 3. 非法字符 `...`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/src.../build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 4. 非法字符 `~`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/s~c/~rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 5. 其他非法字符 `"*\|/<>?:`
        let path = "//crv/cli/s?rc/...~rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);
    }

    #[test]
    fn test_regex_depot_wildcard() {
        // 1. 正确解析
        let path = r"r://\.rs$";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), r"r://\.rs$");

        let path = r"r://^crv/cli/.*\.rs$";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), r"r://^crv/cli/.*\.rs$");

        let path = "r:///crv/cli/src/build.rs"; // 正则表达式是 `/crv/cli/src/build.rs`，虽然什么都匹配不了，但是语法是对的
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "r:///crv/cli/src/build.rs");

        let path = "r://crv/cli/src/build.rs "; // 正则表达式是 `crv/cli/src/build.rs `，虽然什么都匹配不了，但是语法是对的
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "r://crv/cli/src/build.rs ");

        // 2. 语法错误
        let path = "regex://crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "r:://crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "r::/crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 3. 正则错误
        let path = "r://[a-";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::RegexError(_)));
        println!("{}:{}", path, depot_path_err);
    }
}

#[cfg(test)]
mod test_local_path {
    use super::*;

    #[test]
    fn test_local_dir_parse() {
        let path = r"C:\Users/Documents/";
        let local_dir = LocalDir::parse(path);
        assert!(local_dir.is_ok());
        let local_dir = local_dir.unwrap();
        assert_eq!(local_dir.to_unix_path_string(), "/C/Users/Documents/");

        let path = r"C:\Users/Documents\";
        let local_dir = LocalDir::parse(path);
        assert!(local_dir.is_ok());
        let local_dir = local_dir.unwrap();
        assert_eq!(local_dir.to_unix_path_string(), "/C/Users/Documents/");
    }

    #[test]
    fn test_local_path_parse_windows() {
        // 1. Windows 路径 - 基本格式
        let path = r"C:\Users\Documents\file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["C", "Users", "Documents"]);
        assert_eq!(local_path.file, "file.txt");
        assert_eq!(
            local_path.to_unix_path_string(),
            "/C/Users/Documents/file.txt".to_string()
        );

        // 2. Windows 路径 - 使用正斜杠
        let path = "D:/Projects/rust/main.rs";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["D", "Projects", "rust"]);
        assert_eq!(local_path.file, "main.rs");
        assert_eq!(
            local_path.to_unix_path_string(),
            "/D/Projects/rust/main.rs".to_string()
        );

        // 3. Windows 路径 - 混合斜杠
        let path = r"E:\Work/code\src/lib.rs";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["E", "Work", "code", "src"]);
        assert_eq!(local_path.file, "lib.rs");
        assert_eq!(
            local_path.to_unix_path_string(),
            "/E/Work/code/src/lib.rs".to_string()
        );

        // 4. Windows 路径 - 小写盘符
        let path = r"c:\temp\test.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["c", "temp"]);
        assert_eq!(local_path.file, "test.txt");
        assert_eq!(
            local_path.to_unix_path_string(),
            "/c/temp/test.txt".to_string()
        );

        // 5. Windows 路径 - 根目录文件
        let path = r"Z:\readme.md";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["Z"]);
        assert_eq!(local_path.file, "readme.md");
        assert_eq!(local_path.to_unix_path_string(), "/Z/readme.md".to_string());

        // 6. Windows 路径 - 中文路径
        let path = r"C:\用户\文档\新建文本文档.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["C", "用户", "文档"]);
        assert_eq!(local_path.file, "新建文本文档.txt");
        assert_eq!(
            local_path.to_unix_path_string(),
            "/C/用户/文档/新建文本文档.txt".to_string()
        );

        // 7. Windows 路径 - 带空格
        let path = r"C:\Program Files\My App\config.json";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["C", "Program Files", "My App"]);
        assert_eq!(local_path.file, "config.json");
        assert_eq!(
            local_path.to_unix_path_string(),
            "/C/Program Files/My App/config.json".to_string()
        );
    }

    #[test]
    fn test_local_path_parse_unix() {
        // 1. Unix 路径 - 基本格式
        let path = "/home/user/documents/file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["home", "user", "documents"]);
        assert_eq!(local_path.file, "file.txt");

        // 2. Unix 路径 - 根目录文件
        let path = "/etc/hosts";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["etc"]);
        assert_eq!(local_path.file, "hosts");

        // 3. Unix 路径 - 深层嵌套
        let path = "/var/log/nginx/access.log";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["var", "log", "nginx"]);
        assert_eq!(local_path.file, "access.log");

        // 4. Unix 路径 - 带空格
        let path = "/home/user/My Documents/report.pdf";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["home", "user", "My Documents"]);
        assert_eq!(local_path.file, "report.pdf");

        // 5. Unix 路径 - 中文路径
        let path = "/home/用户/文档/测试.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["home", "用户", "文档"]);
        assert_eq!(local_path.file, "测试.txt");

        // 6. Unix 路径 - 特殊字符（合法的）
        let path = "/opt/app-v1.2.3/config_prod.yaml";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["opt", "app-v1.2.3"]);
        assert_eq!(local_path.file, "config_prod.yaml");
    }

    #[test]
    fn test_local_path_parse_errors() {
        // 1. 空路径
        let path = "";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Empty path: {}", e);
        }

        // 2. 相对路径（不是绝对路径）
        let path = "relative/path/file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            println!("Relative path: {}", e);
        }

        // 3. Windows - 无效盘符格式
        let path = "C/Users/file.txt"; // 缺少冒号
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            println!("Invalid drive: {}", e);
        }

        // 4. Unix - 以目录分隔符结尾（无文件名）
        let path = "/home/user/documents/";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            println!("Trailing slash: {}", e);
        }

        // 5. Windows - 以目录分隔符结尾
        let path = r"C:\Users\Documents\";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            println!("Windows trailing slash: {}", e);
        }

        // 6. 连续的分隔符
        let path = "/home//user/file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            println!("Double slash: {}", e);
        }

        // 7. 包含保留字符 `...`
        let path = "/home/user/.../file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Reserved pattern: {}", e);
        }

        // 8. 包含保留字符 `~`
        let path = "/home/user/~test/file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Reserved char ~: {}", e);
        }

        // 9. 包含非法字符（depot path 定义的非法字符）
        let path = r"C:\Users\test|file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Illegal char |: {}", e);
        }

        // 10. 文件名以空格结尾
        let path = "/home/user/file.txt ";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Trailing whitespace: {}", e);
        }

        // 11. Windows - 文件名以空格结尾
        let path = r"C:\Users\file.txt ";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Windows trailing whitespace: {}", e);
        }

        // 12. 包含通配符字符
        let path = "/home/user/*.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::SyntaxError(_)));
            println!("Wildcard char: {}", e);
        }

        // 13. Windows - 无效盘符（多个字符）
        let path = "ABC:\\file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            println!("Invalid drive letter: {}", e);
        }
    }

    #[test]
    fn test_local_path_edge_cases() {
        // 1. 单字母文件名
        let path = "/a";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, Vec::<String>::new());
        assert_eq!(local_path.file, "a");

        // 2. Windows - 单字母文件名
        let path = "C:\\x";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.dirs.0, vec!["C"]);
        assert_eq!(local_path.file, "x");

        // 3. 很长的路径
        let path = "/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.file, "z.txt");
        assert_eq!(local_path.dirs.0.len(), 25);

        // 4. 文件名有多个扩展名
        let path = "/home/user/archive.tar.gz";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.file, "archive.tar.gz");

        // 5. 隐藏文件（Unix）
        let path = "/home/user/.bashrc";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.file, ".bashrc");

        // 6. 没有扩展名的文件
        let path = "/usr/bin/python3";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_ok());
        let local_path = local_path.unwrap();
        assert_eq!(local_path.file, "python3");
    }

    #[test]
    fn test_local_path_wildcard_parse() {
        // 1. 正确解析
        let path = "/crv/cli/src/build.rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "/crv/cli/src/build.rs");

        let path = "/crv/cli/src/";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "/crv/cli/src/");

        let path = "/crv/cli/src/...";
        let depot_path = LocalPathWildcard::parse(path);
        println!("{:?}", depot_path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "/crv/cli/src/...");

        let path = "D:\\crv/cli/src\\...~txt.meta";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "/D/crv/cli/src/...~txt.meta");

        let path = "/crv/cli/src/~.meta";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_custom_string(), "/crv/cli/src/~.meta");

        // 2. 语法错误
        let path = "//crv/cli/src/build.rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "/crv//cli/src/build.rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 文件名以空格结尾
        let path = "/crv/cli/src/新建文本文档.txt ";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 3. 非法字符 `...`，已经是范围索引 depot path 的保留字
        let path = "/crv/cli/src.../build.rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 4. 非法字符 `~`，已经是范围索引 depot path 的保留字
        let path = "/crv/cli/s~c/~rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);

        // 5. 其他非法字符 `"*\|/<>?:`
        let path = "/crv/cli/s?rc/...~rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::SyntaxError(_)));
        println!("{}:{}", path, depot_path_err);
    }
}
