use crate::parsers::path;
use crate::path::basic::LocalDir;
use crate::workspace::entity::*;
use chumsky::prelude::*;

fn workspace_mappings_parser<'src>(
    root_dir: &LocalDir,
) -> impl Parser<'src, &'src str, Vec<(WorkspaceMapping, Option<String>)>, extra::Err<Rich<'src, char>>>
{
    let whitespace = any()
        .filter(|c: &char| c.is_whitespace())
        .repeated()
        .at_least(1);

    let exclude_range = just("-")
        .ignore_then(path::depot_path_wildcard_parser())
        .map(|depot_range| (WorkspaceMapping::Exclude(ExcludeMapping(depot_range)), None));

    // 在解析到 include mapping 时，将 workspace path/dir 中的 workspace name 记录下来，
    // 以便后续验证其正确性
    let include_file = path::depot_path_parser()
        .then_ignore(whitespace)
        .then(path::workspace_path_parser())
        .map(|(depot_file, workspace_file)| {
            (
                WorkspaceMapping::Include(IncludeMapping::File(FileMapping {
                    depot_file,
                    local_file: workspace_file.to_local_path_uncheck(root_dir),
                })),
                Some(workspace_file.workspace_name),
            )
        });
    let include_range = path::range_depot_wildcard_parser()
        .then_ignore(whitespace)
        .then(path::workspace_dir_parser())
        .map(|(depot_folder, workspace_folder)| {
            (
                WorkspaceMapping::Include(IncludeMapping::Folder(FolderMapping {
                    depot_folder,
                    local_folder: workspace_folder.to_local_dir_uncheck(root_dir),
                })),
                Some(workspace_folder.workspace_name),
            )
        });

    choice((exclude_range, include_file, include_range))
        .padded()
        .repeated()
        .collect::<Vec<(WorkspaceMapping, Option<String>)>>()
}

pub fn workspace_mappings(
    input: &str,
    root_dir: &LocalDir,
    workspace_name: &str,
) -> WorkspaceResult<Vec<WorkspaceMapping>> {
    let result = workspace_mappings_parser(root_dir)
        .then_ignore(end().labelled("end of mappings"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            WorkspaceError::SyntaxError(format!("{}", error))
        })?;

    let error_workspace_name = result
        .iter()
        .filter(|(_, name)| name.is_some())
        .filter(|(_, name)| name.as_ref().unwrap() != workspace_name)
        .map(|(_, name)| name.as_ref().unwrap().as_str())
        .collect::<Vec<_>>();

    if !error_workspace_name.is_empty() {
        return Err(WorkspaceError::WorkspaceNameInvalid(
            error_workspace_name.join(", "),
        ));
    }

    let result = result
        .into_iter()
        .map(|(mapping, _)| mapping)
        .collect::<Vec<_>>();

    Ok(result)
}
