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

fn depot_path_parser<'src>() -> impl Parser<'src, &'src str, DepotPath, extra::Err<Rich<'src, char>>>
{
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
            .map(|(_, extension): (_, &str)| FilenameWildcard::Extension(format!(".{extension}"))),
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

fn local_dir_parser<'src>() -> impl Parser<'src, &'src str, LocalDir, extra::Err<Rich<'src, char>>>
{
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

fn local_path_parser<'src>() -> impl Parser<'src, &'src str, LocalPath, extra::Err<Rich<'src, char>>>
{
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
            .map(|(_, extension): (_, &str)| FilenameWildcard::Extension(format!(".{extension}"))),
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
