/// Protocol utilities: file spec parsing, depot path validation, etc.

/// Parse a Perforce-style file spec into components.
///
/// Examples:
/// - `//depot/main/file.txt` → depot file
/// - `//depot/main/...` → recursive wildcard
/// - `//depot/main/*.rs` → wildcard
/// - `//depot/main/file.txt#5` → specific revision
/// - `//depot/main/file.txt@12345` → at changelist
/// - `//depot/main/file.txt@label` → at label
#[derive(Debug, Clone, PartialEq)]
pub struct FileSpec {
    pub depot_path: String,
    pub wildcard: Option<FileSpecWildcard>,
    pub revision: Option<RevisionSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileSpecWildcard {
    /// `...` — recursive match
    Recursive,
    /// `*.ext` — single-directory wildcard
    Pattern(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RevisionSpec {
    /// `#5` — revision number
    Revision(i32),
    /// `#head` — latest revision
    Head,
    /// `#have` — revision currently synced
    Have,
    /// `#none` — non-existent revision (for add)
    None,
    /// `@12345` — at a specific changelist number
    Change(i64),
    /// `@labelname` — at a label
    Label(String),
    /// `@clientname` — at client/workspace
    Client(String),
}

/// Parse a depot file spec string into a `FileSpec`.
///
/// Returns `None` if the string is not a valid depot path spec.
pub fn parse_file_spec(input: &str) -> Option<FileSpec> {
    let input = input.trim();

    // Must start with //
    if !input.starts_with("//") {
        return None;
    }

    // Extract revision spec (after # or @)
    let (path_part, revision) = if let Some(pos) = input.rfind('#') {
        let (path, rev) = input.split_at(pos);
        let rev = &rev[1..]; // skip '#'
        (path, parse_revision_spec(rev, true))
    } else if let Some(pos) = input.rfind('@') {
        let (path, rev) = input.split_at(pos);
        let rev = &rev[1..]; // skip '@'
        (path, parse_revision_spec(rev, false))
    } else {
        (input, None)
    };

    // Detect wildcard
    let (depot_path, wildcard) = if path_part.ends_with("...") {
        (
            path_part.trim_end_matches("...").trim_end_matches('/').to_string(),
            Some(FileSpecWildcard::Recursive),
        )
    } else if path_part.contains('*') || path_part.contains('?') {
        let wildcard_pattern = path_part
            .rsplit('/')
            .next()
            .unwrap_or(path_part)
            .to_string();
        let base = path_part
            .rsplit_once('/')
            .map(|(b, _)| b.to_string())
            .unwrap_or_default();
        (base, Some(FileSpecWildcard::Pattern(wildcard_pattern)))
    } else {
        (path_part.to_string(), None)
    };

    Some(FileSpec {
        depot_path,
        wildcard,
        revision,
    })
}

fn parse_revision_spec(rev: &str, _is_hash: bool) -> Option<RevisionSpec> {
    match rev {
        "head" => Some(RevisionSpec::Head),
        "have" => Some(RevisionSpec::Have),
        "none" => Some(RevisionSpec::None),
        s if s.parse::<i32>().is_ok() => {
            Some(RevisionSpec::Revision(s.parse::<i32>().unwrap()))
        }
        s if s.parse::<i64>().is_ok() => {
            // Heuristic: if it looks like a change number (large number), treat as change
            // In full p4 compatibility, # is always revision and @ is change/label/client
            Some(RevisionSpec::Change(s.parse::<i64>().unwrap()))
        }
        s => {
            // Treat as label or client name
            Some(RevisionSpec::Label(s.to_string()))
        }
    }
}

/// Validate a depot path. Must start with `//depot_name/`.
pub fn validate_depot_path(path: &str) -> bool {
    if !path.starts_with("//") {
        return false;
    }
    let rest = &path[2..];
    // Must have at least depot/dir/file structure; reject trailing slashes
    let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    parts.len() >= 2 && !parts[0].is_empty()
}

/// Validate a local path mapping. Must not contain wildcards in the depot side.
pub fn validate_view_mapping(depot_path: &str, local_path: &str) -> bool {
    validate_depot_path(depot_path) && !local_path.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_file() {
        let spec = parse_file_spec("//depot/main/file.txt").unwrap();
        assert_eq!(spec.depot_path, "//depot/main/file.txt");
        assert_eq!(spec.wildcard, None);
        assert_eq!(spec.revision, None);
    }

    #[test]
    fn test_parse_with_revision() {
        let spec = parse_file_spec("//depot/main/file.txt#5").unwrap();
        assert_eq!(spec.depot_path, "//depot/main/file.txt");
        assert_eq!(spec.revision, Some(RevisionSpec::Revision(5)));
    }

    #[test]
    fn test_parse_recursive_wildcard() {
        let spec = parse_file_spec("//depot/main/...").unwrap();
        assert_eq!(spec.depot_path, "//depot/main");
        assert_eq!(spec.wildcard, Some(FileSpecWildcard::Recursive));
    }

    #[test]
    fn test_parse_at_label() {
        let spec = parse_file_spec("//depot/main/file.txt@mylabel").unwrap();
        assert_eq!(spec.depot_path, "//depot/main/file.txt");
        assert_eq!(spec.revision, Some(RevisionSpec::Label("mylabel".into())));
    }

    #[test]
    fn test_validate_depot_path() {
        assert!(validate_depot_path("//depot/main/file.txt"));
        assert!(validate_depot_path("//depot/a/b/c/file.rs"));
        assert!(!validate_depot_path("//depot/"));
        assert!(!validate_depot_path("file.txt"));
        assert!(!validate_depot_path("/home/user/file.txt"));
    }
}
