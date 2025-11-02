use crate::parsers::path;
use crate::workspace::entity::*;
use chumsky::prelude::*;

fn workspace_mappings_parser<'src>()
-> impl Parser<'src, &'src str, Vec<WorkspaceMapping>, extra::Err<Rich<'src, char>>> {
    let whitespace = any()
        .filter(|c: &char| c.is_whitespace())
        .repeated()
        .at_least(1);

    let exclude_file = just("-")
        .ignore_then(path::depot_path_parser())
        .map(|depot_file| WorkspaceMapping::Exclude(ExcludeMapping::File(depot_file)));
    let exclude_range = just("-")
        .ignore_then(path::depot_path_wildcard_parser())
        .map(|depot_range| WorkspaceMapping::Exclude(ExcludeMapping::Range(depot_range)));

    let include_file = path::depot_path_parser()
        .then_ignore(whitespace)
        .then(path::local_path_parser())
        .map(|(depot_file, local_file)| {
            WorkspaceMapping::Include(IncludeMapping::File(FileMapping {
                depot_file,
                local_file,
            }))
        });
    let include_range = path::range_depot_wildcard_parser()
        .then_ignore(whitespace)
        .then(path::local_dir_parser())
        .map(|(depot_folder, local_folder)| {
            WorkspaceMapping::Include(IncludeMapping::Range(FolderMapping {
                depot_folder,
                local_folder,
            }))
        });

    choice((exclude_file, exclude_range, include_file, include_range))
        .padded()
        .repeated()
        .collect::<Vec<WorkspaceMapping>>()
}

pub fn workspace_mappings(input: &str) -> WorkspaceResult<Vec<WorkspaceMapping>> {
    let result = workspace_mappings_parser()
        .then_ignore(end().labelled("end of mappings"))
        .parse(input)
        .into_result()
        .map_err(|errors| {
            let error = errors.into_iter().next().unwrap();
            WorkspaceError::SyntaxError(format!("{}", error))
        })?;

    Ok(result)
}
