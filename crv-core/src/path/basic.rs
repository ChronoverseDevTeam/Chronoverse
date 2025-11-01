use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PathError {
    #[error("Syntax error: {0}")]
    SyntaxError(String),

    #[error("Invalid path format: {0}")]
    InvalidPath(String),

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilenameWildcard {
    /// 确切的文件名
    Exact(String),
    /// 后缀名通配: ~png, ~obj.meta, 将 `~` 替换为 `.`
    Extension(String),
    /// 匹配所有文件
    All,
}

impl FilenameWildcard {
    fn to_string(&self) -> String {
        match self {
            FilenameWildcard::Exact(s) => s.clone(),
            FilenameWildcard::Extension(s) => format!("~{}", &s[1..]),
            FilenameWildcard::All => String::new(),
        }
    }
}

/// Depot Path (具体的文件)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepotPath {
    pub dirs: Vec<String>,
    pub file: String,
}

impl DepotPath {
    pub fn to_string(&self) -> String {
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
        parser::depot_path(path)
    }
}

/// 通配 Depot Path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DepotPathWildcard {
    /// 范围索引路径: //dir/...~png、//dir/~jpg.meta、//dir/...
    Range(RangeDepotWildcard),
    /// 正则表达式路径: r://project/.*\.rs
    Regex(RegexDepotWildcard),
}

impl DepotPathWildcard {
    pub fn to_string(&self) -> String {
        match self {
            DepotPathWildcard::Range(range_depot_wildcard) => {
                let filename_wildcard_string = range_depot_wildcard.wildcard.to_string();

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
        parser::depot_path_wildcard(wildcard)
    }
}

/// 范围索引 Depot Path
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeDepotWildcard {
    /// 目录部分
    pub dirs: Vec<String>,
    /// 是否包含递归通配符 ...
    pub recursive: bool,
    /// 文件通配符
    pub wildcard: FilenameWildcard,
}

/// 正则 Depot Path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegexDepotWildcard {
    /// 原始正则表达式字符串
    pub pattern: String,
}

/// 本地目录路径（规范化后的绝对路径，精确到目录）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalDir(pub Vec<String>);

impl LocalDir {
    pub fn parse(path: &str) -> PathResult<Self> {
        parser::local_dir(path)
    }
}

/// 本地路径（规范化后的绝对路径，精确到文件）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        parser::local_path(path)
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
    pub fn to_string(&self) -> String {
        let filename_wildcard_string = self.wildcard.to_string();

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
        parser::local_path_wildcard(wildcard)
    }
}

mod parser {
    use crate::path::basic::*;
    use chumsky::prelude::*;

    // 非法字符集合
    const ILLEGAL_CHARS: &str = r#"*|\<>?:"#;

    /// 验证路径段是否包含保留字符或非法字符
    fn validate_segment(s: &str) -> PathResult<String> {
        if s.is_empty() {
            return Err(PathError::SyntaxError(
                "Empty path segment not allowed".to_string(),
            ));
        }

        if s.contains("...") {
            return Err(PathError::InvalidPath(format!(
                "Path segment '{}' contains reserved pattern '...'",
                s
            )));
        }

        if s.contains('~') {
            return Err(PathError::InvalidPath(format!(
                "Path segment '{}' contains reserved character '~'",
                s
            )));
        }

        // 检查其他非法字符
        if let Some(illegal_char) = s.chars().find(|c| ILLEGAL_CHARS.contains(*c)) {
            return Err(PathError::InvalidPath(format!(
                "Path segment '{}' contains illegal character '{}'",
                s, illegal_char
            )));
        }

        Ok(s.to_string())
    }

    fn validate_filename(s: &str) -> PathResult<String> {
        validate_segment(s)?;

        // 额外验证：文件名不能以空白字符结尾
        if s.chars()
            .last()
            .expect("parser should reject empty filename")
            .is_whitespace()
        {
            return Err(PathError::InvalidPath(
                "Filename cannot end with whitespace".to_string(),
            ));
        }
        Ok(s.to_string())
    }

    fn depot_path_parser<'src>()
    -> impl Parser<'src, &'src str, DepotPath, extra::Err<Rich<'src, char>>> {
        // 解析一个路径段（目录名或文件名）
        let path_segment = none_of("/\n\r")
            .repeated()
            .at_least(1)
            .to_slice()
            .labelled("path segment");

        just("//")
            .labelled("depot path prefix '//'")
            .then(
                path_segment
                    .separated_by(just("/"))
                    .at_least(1)
                    .collect::<Vec<&str>>()
                    .labelled("path segments"),
            )
            .then_ignore(end().labelled("end of path"))
            .map(|(_, segments)| {
                // 最后一个段是文件名，其余是目录
                let mut parts: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
                let file = parts.pop().unwrap(); // 安全，因为 at_least(1)
                let dirs = parts;

                DepotPath { dirs, file }
            })
    }

    pub fn depot_path(input: &str) -> PathResult<DepotPath> {
        // 第一步：使用 chumsky parser 解析基本结构
        let result = depot_path_parser()
            .parse(input)
            .into_result()
            .map_err(|errors| {
                let error = errors.into_iter().next().unwrap();
                PathError::SyntaxError(format!("{}", error))
            })?;

        // 第二步：验证每个路径段
        for dir in &result.dirs {
            validate_segment(dir)?;
        }
        validate_filename(&result.file)?;

        Ok(result)
    }

    fn depot_path_wildcard_parser<'src>()
    -> impl Parser<'src, &'src str, DepotPathWildcard, extra::Err<Rich<'src, char>>> {
        // 解析一个路径段（目录名）
        let path_segment = none_of("/\n\r")
            .repeated()
            .at_least(1)
            .to_slice()
            .labelled("path segment");

        let filename_wildcard = choice((
            just("~")
                .then(path_segment)
                .map(|(_, extension): (_, &str)| {
                    FilenameWildcard::Extension(format!(".{extension}"))
                }),
            path_segment.map(|filename: &str| FilenameWildcard::Exact(filename.to_string())),
            empty().map(|_| FilenameWildcard::All),
        ));

        let range_depot_wildcard = just("//")
            .labelled("range depot wildcard prefix")
            .then(
                path_segment
                    .then_ignore(just("/"))
                    .repeated()
                    .collect::<Vec<&str>>()
                    .labelled("path segments"),
            )
            .then(just("...").or_not())
            .then(filename_wildcard)
            .then_ignore(end().labelled("end of path"))
            .map(|(((_, segments), recursive_dir), filename)| {
                let dirs = segments.iter().map(|s| s.to_string()).collect();

                DepotPathWildcard::Range(RangeDepotWildcard {
                    dirs,
                    recursive: recursive_dir.is_some(),
                    wildcard: filename,
                })
            });

        let regex_depot_wildcard = just("r://")
            .labelled("regex depot wildcard prefix")
            .then(none_of("\n\r").repeated().collect())
            .then_ignore(end().labelled("end of path"))
            .map(|(_, pattern)| DepotPathWildcard::Regex(RegexDepotWildcard { pattern }));

        choice((range_depot_wildcard, regex_depot_wildcard))
    }

    pub fn depot_path_wildcard(input: &str) -> PathResult<DepotPathWildcard> {
        // 使用 chumsky parser 解析基本结构
        let result = depot_path_wildcard_parser()
            .parse(input)
            .into_result()
            .map_err(|errors| {
                let error = errors.into_iter().next().unwrap();
                PathError::SyntaxError(format!("{}", error))
            })?;

        match &result {
            DepotPathWildcard::Range(range_depot_wildcard) => {
                // 验证每个路径段
                for dir in &range_depot_wildcard.dirs {
                    validate_segment(dir)?;
                }
                // 验证文件名
                match &range_depot_wildcard.wildcard {
                    FilenameWildcard::Exact(exact) => {
                        validate_filename(exact)?;
                    }
                    FilenameWildcard::Extension(extension) => {
                        validate_filename(extension)?;
                    }
                    FilenameWildcard::All => {}
                }
            }
            DepotPathWildcard::Regex(regex_depot_wildcard) => {
                // 验证 regex
                let regex_compile_result = regex::Regex::new(&regex_depot_wildcard.pattern);
                if regex_compile_result.is_err() {
                    return Err(PathError::RegexError(regex_compile_result.err().unwrap()));
                }
            }
        };

        Ok(result)
    }

    fn local_dir_parser<'src>()
    -> impl Parser<'src, &'src str, LocalDir, extra::Err<Rich<'src, char>>> {
        // 解析一个路径段（目录名）
        let path_segment = none_of("\\/\n\r")
            .repeated()
            .at_least(1)
            .to_slice()
            .labelled("path segment");

        // 路径分隔符
        let path_separator = one_of("/\\").labelled("path separator");

        // 解析盘符
        let dirve_letter = any()
            .filter(|c: &char| c.is_ascii_alphabetic())
            .then_ignore(just(":"))
            .labelled("drive letter");

        choice((
            dirve_letter.map(|c| Some(c.to_string())),
            just("/").map(|_| None),
        ))
        .then(
            path_segment
                .separated_by(path_separator)
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .then_ignore(path_separator.or_not())
        .then_ignore(end().labelled("end of path"))
        .map(|(dirve_letter, segments)| {
            let mut dirs = if let Some(dirve_letter) = dirve_letter {
                vec![dirve_letter.to_string()]
            } else {
                vec![]
            };
            dirs.extend(segments.iter().map(|s| s.to_string()));
            LocalDir(dirs)
        })
    }

    pub fn local_dir(input: &str) -> PathResult<LocalDir> {
        // 第一步：使用 chumsky parser 解析基本结构
        let result = local_dir_parser()
            .parse(input)
            .into_result()
            .map_err(|errors| {
                let error = errors.into_iter().next().unwrap();
                PathError::SyntaxError(format!("{}", error))
            })?;

        // 第二步：验证每个路径段
        for dir in &result.0 {
            validate_segment(dir)?;
        }

        Ok(result)
    }

    fn local_path_parser<'src>()
    -> impl Parser<'src, &'src str, LocalPath, extra::Err<Rich<'src, char>>> {
        // 解析一个路径段（目录名或文件名）
        let path_segment = none_of("\\/\n\r")
            .repeated()
            .at_least(1)
            .to_slice()
            .labelled("path segment");

        // 路径分隔符
        let path_separator = one_of("/\\").labelled("path separator");

        // 解析盘符
        let dirve_letter = any()
            .filter(|c: &char| c.is_ascii_alphabetic())
            .then_ignore(just(":"))
            .then_ignore(path_separator)
            .labelled("drive letter");

        choice((
            dirve_letter.map(|c| Some(c.to_string())),
            just("/").map(|_| None),
        ))
        .then(
            path_segment
                .separated_by(path_separator)
                .at_least(1)
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .then_ignore(end().labelled("end of path"))
        .map(|(dirve_letter, segments)| {
            let mut dirs = if let Some(dirve_letter) = dirve_letter {
                vec![dirve_letter.to_string()]
            } else {
                vec![]
            };
            // 最后一个段是文件名，其余是目录
            let mut parts: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
            let file = parts.pop().unwrap(); // 安全，因为 at_least(1)
            dirs.append(&mut parts);

            LocalPath {
                dirs: LocalDir(dirs),
                file,
            }
        })
    }

    pub fn local_path(input: &str) -> PathResult<LocalPath> {
        // 第一步：使用 chumsky parser 解析基本结构
        let result = local_path_parser()
            .parse(input)
            .into_result()
            .map_err(|errors| {
                let error = errors.into_iter().next().unwrap();
                PathError::SyntaxError(format!("{}", error))
            })?;

        // 第二步：验证每个路径段
        for dir in &result.dirs.0 {
            validate_segment(dir)?;
        }
        validate_filename(&result.file)?;

        Ok(result)
    }

    fn local_path_wildcard_parser<'src>()
    -> impl Parser<'src, &'src str, LocalPathWildcard, extra::Err<Rich<'src, char>>> {
        // 解析一个路径段（目录名或文件名）
        let path_segment = none_of("\\/\n\r")
            .repeated()
            .at_least(1)
            .to_slice()
            .labelled("path segment");

        let filename_wildcard = choice((
            just("~")
                .then(path_segment)
                .map(|(_, extension): (_, &str)| {
                    FilenameWildcard::Extension(format!(".{extension}"))
                }),
            path_segment.map(|filename: &str| FilenameWildcard::Exact(filename.to_string())),
            empty().map(|_| FilenameWildcard::All),
        ));

        // 路径分隔符
        let path_separator = one_of("/\\").labelled("path separator");

        // 解析盘符
        let dirve_letter = any()
            .filter(|c: &char| c.is_ascii_alphabetic())
            .then_ignore(just(":"))
            .then_ignore(path_separator)
            .labelled("drive letter");

        choice((
            dirve_letter.map(|c| Some(c.to_string())),
            just("/").map(|_| None),
        ))
        .then(
            path_segment
                .then_ignore(path_separator)
                .repeated()
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .then(just("...").or_not())
        .then(filename_wildcard)
        .then_ignore(end().labelled("end of path"))
        .map(|(((dirve_letter, segments), recursive_dir), filename)| {
            let mut dirs = if let Some(dirve_letter) = dirve_letter {
                vec![dirve_letter.to_string()]
            } else {
                vec![]
            };
            dirs.extend(segments.iter().map(|s| s.to_string()));

            LocalPathWildcard {
                dirs: LocalDir(dirs),
                recursive: recursive_dir.is_some(),
                wildcard: filename,
            }
        })
    }

    pub fn local_path_wildcard(input: &str) -> PathResult<LocalPathWildcard> {
        // 使用 chumsky parser 解析基本结构
        let result = local_path_wildcard_parser()
            .parse(input)
            .into_result()
            .map_err(|errors| {
                let error = errors.into_iter().next().unwrap();
                PathError::SyntaxError(format!("{}", error))
            })?;

        // 验证每个路径段
        for dir in &result.dirs.0 {
            validate_segment(dir)?;
        }
        // 验证文件名
        match &result.wildcard {
            FilenameWildcard::Exact(exact) => {
                validate_filename(exact)?;
            }
            FilenameWildcard::Extension(extension) => {
                validate_filename(extension)?;
            }
            FilenameWildcard::All => {}
        }

        Ok(result)
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
        assert_eq!(depot_path.to_string(), "//crv/cli/src/build.rs");

        let path = "//crv/cli/src/新建文本文档.txt";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "//crv/cli/src/新建文本文档.txt");

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
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        let path = "//crv/cli/src/...";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 4. 非法字符 `~`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/src/~build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 5. 其他非法字符 `"*\|/<>?:`
        let path = "//crv/cli/s?rc/build.rs";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 6. 文件名以空格结尾
        let path = "//crv/cli/src/新建文本文档.txt ";
        let depot_path = DepotPath::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);
    }

    #[test]
    fn test_range_depot_wildcard_parse() {
        // 1. 正确解析
        let path = "//crv/cli/src/build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "//crv/cli/src/build.rs");

        let path = "//crv/cli/src/";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "//crv/cli/src/");

        let path = "//crv/cli/src/...";
        let depot_path = DepotPathWildcard::parse(path);
        println!("{:?}", depot_path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "//crv/cli/src/...");

        let path = "//crv/cli/src/...~txt.meta";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "//crv/cli/src/...~txt.meta");

        let path = "//crv/cli/src/~.meta";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "//crv/cli/src/~.meta");

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

        // 3. 非法字符 `...`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/src.../build.rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 4. 非法字符 `~`，已经是范围索引 depot path 的保留字
        let path = "//crv/cli/s~c/~rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 5. 其他非法字符 `"*\|/<>?:`
        let path = "//crv/cli/s?rc/...~rs";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 6. 文件名以空格结尾
        let path = "//crv/cli/src/新建文本文档.txt ";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);
    }

    #[test]
    fn test_regex_depot_wildcard() {
        // 1. 正确解析
        let path = r"r://\.rs$";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), r"r://\.rs$");

        let path = r"r://^crv/cli/.*\.rs$";
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), r"r://^crv/cli/.*\.rs$");

        let path = "r:///crv/cli/src/build.rs"; // 正则表达式是 `/crv/cli/src/build.rs`，虽然什么都匹配不了，但是语法是对的
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "r:///crv/cli/src/build.rs");

        let path = "r://crv/cli/src/build.rs "; // 正则表达式是 `crv/cli/src/build.rs `，虽然什么都匹配不了，但是语法是对的
        let depot_path = DepotPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "r://crv/cli/src/build.rs ");

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
            assert!(matches!(
                e,
                PathError::SyntaxError(_) | PathError::InvalidPath(_)
            ));
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
            assert!(matches!(e, PathError::InvalidPath(_)));
            println!("Reserved pattern: {}", e);
        }

        // 8. 包含保留字符 `~`
        let path = "/home/user/~test/file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::InvalidPath(_)));
            println!("Reserved char ~: {}", e);
        }

        // 9. 包含非法字符（depot path 定义的非法字符）
        let path = r"C:\Users\test|file.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::InvalidPath(_)));
            println!("Illegal char |: {}", e);
        }

        // 10. 文件名以空格结尾
        let path = "/home/user/file.txt ";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::InvalidPath(_)));
            println!("Trailing whitespace: {}", e);
        }

        // 11. Windows - 文件名以空格结尾
        let path = r"C:\Users\file.txt ";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::InvalidPath(_)));
            println!("Windows trailing whitespace: {}", e);
        }

        // 12. 包含通配符字符
        let path = "/home/user/*.txt";
        let local_path = LocalPath::parse(path);
        assert!(local_path.is_err());
        if let Err(e) = local_path {
            assert!(matches!(e, PathError::InvalidPath(_)));
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
        assert_eq!(depot_path.to_string(), "/crv/cli/src/build.rs");

        let path = "/crv/cli/src/";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "/crv/cli/src/");

        let path = "/crv/cli/src/...";
        let depot_path = LocalPathWildcard::parse(path);
        println!("{:?}", depot_path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "/crv/cli/src/...");

        let path = "D:\\crv/cli/src\\...~txt.meta";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "/D/crv/cli/src/...~txt.meta");

        let path = "/crv/cli/src/~.meta";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_ok());
        let depot_path = depot_path.unwrap();
        assert_eq!(depot_path.to_string(), "/crv/cli/src/~.meta");

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

        // 3. 非法字符 `...`，已经是范围索引 depot path 的保留字
        let path = "/crv/cli/src.../build.rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 4. 非法字符 `~`，已经是范围索引 depot path 的保留字
        let path = "/crv/cli/s~c/~rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 5. 其他非法字符 `"*\|/<>?:`
        let path = "/crv/cli/s?rc/...~rs";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);

        // 6. 文件名以空格结尾
        let path = "/crv/cli/src/新建文本文档.txt ";
        let depot_path = LocalPathWildcard::parse(path);
        assert!(depot_path.is_err());
        let depot_path_err = depot_path.err().unwrap();
        assert!(matches!(depot_path_err, PathError::InvalidPath(_)));
        println!("{}:{}", path, depot_path_err);
    }
}
