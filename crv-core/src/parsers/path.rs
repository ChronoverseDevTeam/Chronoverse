use crate::path::basic::*;
use chumsky::prelude::*;

// 路段中的非法字符集合
// const SEGMENT_ILLEGAL_CHARS: &str = "~*\\|/<>?:\r\n";
const SEGMENT_ILLEGAL_CHARS_WITH_BLANK: &str = " ~*\\|/<>?:\r\n";

pub fn depot_path_parser<'src>()
-> impl Parser<'src, &'src str, DepotPath, extra::Err<Rich<'src, char>>> {
    just("//")
        .labelled("depot path prefix '//'")
        .then(
            path_segment_parser()
                .separated_by(just("/"))
                .at_least(1)
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .map(|(_, segments)| {
            // 最后一个段是文件名，其余是目录
            let mut parts: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
            let file = parts.pop().unwrap(); // 安全，因为 at_least(1)
            let dirs = parts;

            DepotPath { dirs, file }
        })
}

pub fn depot_path(input: &str) -> PathResult<DepotPath> {
    // 使用 chumsky parser 解析基本结构
    let result = depot_path_parser()
        .then_ignore(end().labelled("end of depot path"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}

pub fn range_depot_wildcard_parser<'src>()
-> impl Parser<'src, &'src str, RangeDepotWildcard, extra::Err<Rich<'src, char>>> {
    just("//")
        .labelled("range depot wildcard prefix")
        .then(
            path_segment_parser()
                .then_ignore(just("/"))
                .repeated()
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .then(just("...").or_not())
        .then(filename_wildcard_parser())
        .map(|(((_, segments), recursive_dir), filename)| {
            let dirs = segments.iter().map(|s| s.to_string()).collect();

            RangeDepotWildcard {
                dirs,
                recursive: recursive_dir.is_some(),
                wildcard: filename,
            }
        })
}

fn path_segment_parser<'src>()
-> Boxed<'src, 'src, &'src str, &'src str, extra::Err<Rich<'src, char>>> {
    let none_blank_block = none_of(SEGMENT_ILLEGAL_CHARS_WITH_BLANK)
        .repeated()
        .at_least(1)
        .to_slice()
        .filter(|s: &&str| !(*s).contains("..."));
    let blank_block = just(" ").repeated().at_least(1);

    // 解析一个路径段（目录名或文件名），不能以空格开头或结尾
    none_blank_block
        .then(blank_block.then(none_blank_block).repeated())
        .to_slice()
        .labelled("path segment")
        .boxed()
}

fn filename_wildcard_parser<'src>()
-> impl Parser<'src, &'src str, FilenameWildcard, extra::Err<Rich<'src, char>>> {
    choice((
        just("~")
            .then(path_segment_parser())
            .map(|(_, extension): (_, &str)| FilenameWildcard::Extension(format!(".{extension}"))),
        path_segment_parser().map(|filename: &str| FilenameWildcard::Exact(filename.to_string())),
        empty().map(|_| FilenameWildcard::All),
    ))
    .boxed()
}

pub fn depot_path_wildcard_parser<'src>()
-> impl Parser<'src, &'src str, DepotPathWildcard, extra::Err<Rich<'src, char>>> {
    let range_depot_wildcard =
        range_depot_wildcard_parser().map(|range| DepotPathWildcard::Range(range));

    let regex_depot_wildcard = just("r://")
        .labelled("regex depot wildcard prefix")
        .then(none_of("\n\r").repeated().collect())
        .map(|(_, pattern)| DepotPathWildcard::Regex(RegexDepotWildcard { pattern }));

    choice((range_depot_wildcard, regex_depot_wildcard))
}

pub fn depot_path_wildcard(input: &str) -> PathResult<DepotPathWildcard> {
    // 使用 chumsky parser 解析基本结构
    let result = depot_path_wildcard_parser()
        .then_ignore(end().labelled("end of depot path wildcard"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    match &result {
        DepotPathWildcard::Range(_) => {}
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
        path_segment_parser()
            .then_ignore(path_separator.or_not())
            .repeated()
            .collect::<Vec<&str>>()
            .labelled("path segments"),
    )
    .then_ignore(path_segment_parser().not().rewind())
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
    // 使用 chumsky parser 解析基本结构
    let result = local_dir_parser()
        .then_ignore(end().labelled("end of local dir"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}

fn local_path_parser<'src>() -> impl Parser<'src, &'src str, LocalPath, extra::Err<Rich<'src, char>>>
{
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
        path_segment_parser()
            .separated_by(path_separator)
            .at_least(1)
            .collect::<Vec<&str>>()
            .labelled("path segments"),
    )
    .then_ignore(path_separator.not().rewind())
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
    // 使用 chumsky parser 解析基本结构
    let result = local_path_parser()
        .then_ignore(end().labelled("end of local path"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}

fn local_path_wildcard_parser<'src>()
-> impl Parser<'src, &'src str, LocalPathWildcard, extra::Err<Rich<'src, char>>> {
    let filename_wildcard = choice((
        just("~")
            .then(path_segment_parser())
            .map(|(_, extension): (_, &str)| FilenameWildcard::Extension(format!(".{extension}"))),
        path_segment_parser().map(|filename: &str| FilenameWildcard::Exact(filename.to_string())),
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
        path_segment_parser()
            .then_ignore(path_separator)
            .repeated()
            .collect::<Vec<&str>>()
            .labelled("path segments"),
    )
    .then(just("...").or_not())
    .then(filename_wildcard)
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
        .then_ignore(end().labelled("end of local path wildcard"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}

pub fn workspace_path_parser<'src>()
-> impl Parser<'src, &'src str, WorkspacePath, extra::Err<Rich<'src, char>>> {
    just("//")
        .labelled("depot path prefix '//'")
        .then(
            path_segment_parser()
                .separated_by(just("/"))
                .at_least(2)
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .then_ignore(just("/").not().rewind())
        .map(|(_, segments)| {
            let mut parts: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
            // 第一个段是 workspace 名称
            let workspace_name = parts.remove(0);

            // 最后一个段是文件名
            let file = parts.pop().unwrap(); // 安全，因为 at_least(2)
            let dirs = parts;

            WorkspacePath {
                workspace_name,
                dirs,
                file,
            }
        })
}

pub fn workspace_path(input: &str) -> PathResult<WorkspacePath> {
    let result = workspace_path_parser()
        .then_ignore(end().labelled("end of workspace path"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}

pub fn workspace_dir_parser<'src>()
-> impl Parser<'src, &'src str, WorkspaceDir, extra::Err<Rich<'src, char>>> {
    just("//")
        .labelled("depot path prefix '//'")
        .then(
            path_segment_parser()
                .then_ignore(just("/"))
                .repeated()
                .at_least(1)
                .collect::<Vec<&str>>()
                .labelled("path segments"),
        )
        .then_ignore(path_segment_parser().not().rewind())
        .map(|(_, segments)| {
            let mut parts: Vec<String> = segments.iter().map(|s| s.to_string()).collect();
            // 第一个段是 workspace 名称
            let workspace_name = parts.remove(0);

            // 剩下的是目录
            let dirs = parts;

            WorkspaceDir {
                workspace_name,
                dirs,
            }
        })
}

pub fn workspace_dir(input: &str) -> PathResult<WorkspaceDir> {
    let result = workspace_dir_parser()
        .then_ignore(end().labelled("end of workspace dir"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            PathError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}
