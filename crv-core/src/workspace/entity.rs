use crate::{
    parsers,
    path::basic::{
        DepotPath, DepotPathWildcard, LocalDir, LocalPath, LocalPathWildcard, RangeDepotWildcard,
    },
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkspaceError {
    #[error("Syntax error: {0}")]
    SyntaxError(String),

    #[error("Mapping conflict:\n{0}")]
    MappingConflictError(String),

    #[error("Local dir in mapping not under root:\n{0}")]
    MappingNotUnderRoot(String),
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntity {
    #[serde(rename = "_id")]
    pub name: String,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "bson::serde_helpers::chrono_datetime_as_bson_datetime")]
    pub updated_at: DateTime<Utc>,
    pub owner: String,
    pub path: String,
    pub device_finger_print: String,
}

/// Workspace 映射关系
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceMapping {
    Include(IncludeMapping),
    Exclude(ExcludeMapping),
}

/// Workspace 包含映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IncludeMapping {
    File(FileMapping),
    Folder(FolderMapping),
}

/// Workspace 排除映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExcludeMapping(pub DepotPathWildcard);

/// 单文件映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMapping {
    /// Depot 文件
    pub depot_file: DepotPath,
    /// 本地文件
    pub local_file: LocalPath,
}

/// 文件夹映射
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMapping {
    /// Depot 路径范围
    pub depot_folder: RangeDepotWildcard,
    /// 本地文件夹
    pub local_folder: LocalDir,
}

/// Workspace 配置 TODO: 需要接到 WorkspaceEntity 中
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    /// 根目录
    pub root_dir: LocalDir,
    /// 映射列表（按顺序处理，后面的覆盖前面的）
    pub mappings: Vec<WorkspaceMapping>,
}

impl WorkspaceConfig {
    pub fn from_specification(root_dir: &str, mappings: &str) -> WorkspaceResult<Self> {
        let root_dir =
            LocalDir::parse(root_dir).map_err(|e| WorkspaceError::SyntaxError(format!("{}", e)))?;

        let mappings = parsers::workspace::workspace_mappings(mappings)?;

        let workspace_config = Self { root_dir, mappings };

        if let Err(errors) = workspace_config.verify_mapping_under_root() {
            let errors = errors.join("\n");
            return WorkspaceResult::Err(WorkspaceError::MappingNotUnderRoot(errors));
        }

        if let Err(errors) = workspace_config.verify_conflict_free() {
            let errors = errors.join("\n");
            return WorkspaceResult::Err(WorkspaceError::MappingConflictError(errors));
        }

        Ok(workspace_config)
    }

    fn verify_mapping_under_root(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];

        for mapping in &self.mappings {
            match mapping {
                WorkspaceMapping::Include(include_mapping) => {
                    match include_mapping {
                        IncludeMapping::File(file_mapping) => {
                            if Self::common_prefix_end_index(
                                &file_mapping.local_file.dirs.0,
                                &self.root_dir.0,
                            ) == self.root_dir.0.len()
                            {
                                continue;
                            }
                            errors.push(format!(
                                "local file {} not under root dir {}",
                                file_mapping.local_file.to_unix_path_string(),
                                self.root_dir.to_unix_path_string()
                            ));
                        }
                        IncludeMapping::Folder(folder_mapping) => {
                            if Self::common_prefix_end_index(
                                &folder_mapping.local_folder.0,
                                &self.root_dir.0,
                            ) == self.root_dir.0.len()
                            {
                                continue;
                            }
                            errors.push(format!(
                                "local dir {} not under root dir {}",
                                folder_mapping.local_folder.to_unix_path_string(),
                                self.root_dir.to_unix_path_string()
                            ));
                        }
                    };
                }
                WorkspaceMapping::Exclude(_) => {
                    continue;
                }
            }
        }

        if errors.is_empty() {
            return Ok(());
        } else {
            return Err(errors);
        }
    }

    /// 检查 mapping 中是否存在冲突的配置项
    fn verify_conflict_free(&self) -> Result<(), Vec<String>> {
        let mut errors = vec![];

        // 检查映射是否冲突
        for i in 0..self.mappings.len() {
            for j in i + 1..self.mappings.len() {
                if let Err(error_message) = self.verify_mapping_pair(i, j) {
                    errors.push(format!(
                        "Mapping rules {} and {} conflict: {}",
                        i, j, error_message
                    ));
                }
            }
        }

        if errors.is_empty() {
            return Ok(());
        } else {
            return Err(errors);
        }
    }

    /// 仅考虑映射类型的情况下，判断两个映射是否可能冲突，如果可能则按照 primary, secondary 的顺序返回
    fn try_race_mapping_pair<'a>(
        &'a self,
        index_1: usize,
        index_2: usize,
    ) -> Option<(&'a IncludeMapping, &'a IncludeMapping)> {
        if index_1 == index_2 {
            return None;
        }

        let primary = index_1.max(index_2);
        let secondary = index_1.min(index_2);

        let primary = &self.mappings[primary];
        let secondary = &self.mappings[secondary];

        // 只有两个 mapping 都是 include mapping 的时候才有意义
        let primary = match primary {
            WorkspaceMapping::Include(include_mapping) => include_mapping,
            WorkspaceMapping::Exclude(_) => return None,
        };
        let secondary = match secondary {
            WorkspaceMapping::Include(include_mapping) => include_mapping,
            WorkspaceMapping::Exclude(_) => return None,
        };
        return Some((primary, secondary));
    }

    /// 仅考虑后缀名/文件名的情况下，判断两个映射是否可能冲突，如果可能冲突，返回可能发生冲突的本地文件名
    fn possible_conflict_file(&self, index_1: usize, index_2: usize) -> Option<String> {
        let race_pair = self.try_race_mapping_pair(index_1, index_2);
        if race_pair.is_none() {
            return None;
        }
        let (primary, secondary) = race_pair.unwrap();

        match (primary, secondary) {
            // case 1. 都是文件
            (IncludeMapping::File(primary_mapping), IncludeMapping::File(secondary_mapping)) => {
                if primary_mapping.local_file.file == secondary_mapping.local_file.file {
                    return Some(primary_mapping.local_file.file.clone());
                } else {
                    return None;
                }
            }
            // case 2. 一个是文件，另一个是目录
            (IncludeMapping::File(primary_mapping), IncludeMapping::Folder(secondary_mapping)) => {
                match &secondary_mapping.depot_folder.wildcard {
                    crate::path::basic::FilenameWildcard::Exact(filename) => {
                        if &primary_mapping.local_file.file == filename {
                            return Some(primary_mapping.local_file.file.clone());
                        } else {
                            return None;
                        }
                    }
                    crate::path::basic::FilenameWildcard::Extension(extension) => {
                        if primary_mapping.local_file.file.ends_with(extension) {
                            return Some(primary_mapping.local_file.file.clone());
                        } else {
                            return None;
                        }
                    }
                    crate::path::basic::FilenameWildcard::All => {
                        return Some(primary_mapping.local_file.file.clone());
                    }
                }
            }
            // case 2. 一个是文件，另一个是目录
            (IncludeMapping::Folder(primary_mapping), IncludeMapping::File(secondary_mapping)) => {
                match &primary_mapping.depot_folder.wildcard {
                    crate::path::basic::FilenameWildcard::Exact(filename) => {
                        if &secondary_mapping.local_file.file == filename {
                            return Some(secondary_mapping.local_file.file.clone());
                        } else {
                            return None;
                        }
                    }
                    crate::path::basic::FilenameWildcard::Extension(extension) => {
                        if secondary_mapping.local_file.file.ends_with(extension) {
                            return Some(secondary_mapping.local_file.file.clone());
                        } else {
                            return None;
                        }
                    }
                    crate::path::basic::FilenameWildcard::All => {
                        return Some(secondary_mapping.local_file.file.clone());
                    }
                }
            }
            // case 3. 都是目录
            (
                IncludeMapping::Folder(primary_mapping),
                IncludeMapping::Folder(secondary_mapping),
            ) => {
                match (
                    &primary_mapping.depot_folder.wildcard,
                    &secondary_mapping.depot_folder.wildcard,
                ) {
                    (
                        crate::path::basic::FilenameWildcard::Exact(primary_filename),
                        crate::path::basic::FilenameWildcard::Exact(secondary_filename),
                    ) => {
                        if primary_filename == secondary_filename {
                            return Some(primary_filename.clone());
                        } else {
                            return None;
                        }
                    }
                    (
                        crate::path::basic::FilenameWildcard::Exact(filename),
                        crate::path::basic::FilenameWildcard::Extension(extension),
                    ) => {
                        if filename.ends_with(extension) {
                            return Some(filename.clone());
                        } else {
                            return None;
                        }
                    }
                    (
                        crate::path::basic::FilenameWildcard::Exact(filename),
                        crate::path::basic::FilenameWildcard::All,
                    ) => {
                        return Some(filename.clone());
                    }
                    (
                        crate::path::basic::FilenameWildcard::Extension(extension),
                        crate::path::basic::FilenameWildcard::Exact(filename),
                    ) => {
                        if filename.ends_with(extension) {
                            return Some(filename.clone());
                        } else {
                            return None;
                        }
                    }
                    (
                        crate::path::basic::FilenameWildcard::Extension(primary_extension),
                        crate::path::basic::FilenameWildcard::Extension(secondary_extension),
                    ) => {
                        if primary_extension.ends_with(secondary_extension) {
                            return Some(format!("your_file{primary_extension}"));
                        } else if secondary_extension.ends_with(primary_extension) {
                            return Some(format!("your_file{secondary_extension}"));
                        } else {
                            return None;
                        }
                    }
                    (
                        crate::path::basic::FilenameWildcard::Extension(extension),
                        crate::path::basic::FilenameWildcard::All,
                    ) => {
                        return Some(format!("your_file{extension}"));
                    }
                    (
                        crate::path::basic::FilenameWildcard::All,
                        crate::path::basic::FilenameWildcard::Exact(filename),
                    ) => {
                        return Some(filename.clone());
                    }
                    (
                        crate::path::basic::FilenameWildcard::All,
                        crate::path::basic::FilenameWildcard::Extension(extension),
                    ) => {
                        return Some(format!("your_file{extension}"));
                    }
                    (
                        crate::path::basic::FilenameWildcard::All,
                        crate::path::basic::FilenameWildcard::All,
                    ) => {
                        return Some(format!("your_file.txt"));
                    }
                }
            }
        }
    }

    fn conflict_message(
        depot_path_1: &DepotPath,
        depot_path_2: &DepotPath,
        local_path: &LocalPath,
    ) -> String {
        format!(
            "{} and {} will both be mapped to the local path {}",
            depot_path_1.to_string(),
            depot_path_2.to_string(),
            local_path.to_unix_path_string()
        )
    }

    /// 给定两个映射的索引，判断是否存在冲突
    fn verify_mapping_pair(&self, index_1: usize, index_2: usize) -> Result<(), String> {
        let race_pair = self.try_race_mapping_pair(index_1, index_2);
        if race_pair.is_none() {
            return Ok(());
        }
        let (primary, secondary) = race_pair.unwrap();
        let conflict_file_example = match self.possible_conflict_file(index_1, index_2) {
            Some(filename) => filename,
            None => return Ok(()),
        };

        match (primary, secondary) {
            // case 1. 都是文件
            (IncludeMapping::File(primary_mapping), IncludeMapping::File(secondary_mapping)) => {
                let primary_local_file = &primary_mapping.local_file;
                let secondary_local_file = &secondary_mapping.local_file;
                let primary_depot_file = &primary_mapping.depot_file;
                let secondary_depot_file = &secondary_mapping.depot_file;

                // case 1.1. local 相同但是 depot 不同，冲突
                if primary_local_file == secondary_local_file
                    && primary_depot_file != secondary_depot_file
                {
                    // primary_depot_file 与 secondary_depot_file 都会映射到 primary_local_file
                    Err(Self::conflict_message(
                        primary_depot_file,
                        secondary_depot_file,
                        primary_local_file,
                    ))
                } else {
                    // case 1.2. 其他情况，不冲突
                    return Ok(());
                }
            }
            // case 2. primary 是文件，secondary 是目录
            (IncludeMapping::File(primary_mapping), IncludeMapping::Folder(secondary_mapping)) => {
                let primary_local_file = &primary_mapping.local_file;
                let primary_depot_file = &primary_mapping.depot_file;
                let secondary_local_dir = &secondary_mapping.local_folder;
                let secondary_depot_dir = &secondary_mapping.depot_folder;
                let local_common_prefix_end_index = Self::common_prefix_end_index(
                    &primary_local_file.dirs.0,
                    &secondary_local_dir.0,
                );
                let depot_common_prefix_end_index = Self::common_prefix_end_index(
                    &primary_depot_file.dirs,
                    &secondary_depot_dir.dirs,
                );
                let local_contain = local_common_prefix_end_index == secondary_local_dir.0.len();
                let depot_contain = depot_common_prefix_end_index == secondary_depot_dir.dirs.len();

                // case 2.1. local 包含，depot 不包含，冲突
                if local_contain && !depot_contain {
                    let local_diff = &primary_local_file.dirs.0[local_common_prefix_end_index..];
                    // 非递归目录的情况下，当且仅当 local dir 完全相同的时候冲突
                    if !secondary_depot_dir.recursive && !local_diff.is_empty() {
                        return Ok(());
                    }

                    // secondary_depot_dir + local_diff
                    // 与 primary_depot_file
                    // 都会映射到 primary_local_file
                    let mut depot_path_1_dir = Vec::new();
                    depot_path_1_dir.extend_from_slice(&secondary_depot_dir.dirs);
                    depot_path_1_dir.extend_from_slice(local_diff);
                    let depot_path_1 = DepotPath {
                        dirs: depot_path_1_dir,
                        file: conflict_file_example.clone(),
                    };
                    let depot_path_2 = DepotPath {
                        dirs: primary_depot_file.dirs.clone(),
                        file: primary_depot_file.file.clone(),
                    };
                    let local_path = LocalPath {
                        dirs: primary_local_file.dirs.clone(),
                        file: conflict_file_example.clone(),
                    };

                    return Err(Self::conflict_message(
                        &depot_path_1,
                        &depot_path_2,
                        &local_path,
                    ));
                } else if local_contain && depot_contain {
                    let local_diff = &primary_local_file.dirs.0[local_common_prefix_end_index..];
                    let depot_diff = &primary_depot_file.dirs[depot_common_prefix_end_index..];
                    // case 2.2. local 包含，depot 包含
                    // case 2.2.1. 文件部分相同，不冲突
                    if local_diff == depot_diff
                        && primary_local_file.file == primary_depot_file.file
                    {
                        return Ok(());
                    } else {
                        // case 2.2.2. 文件部分不同，冲突
                        // 非递归目录的情况下，当且仅当 local dir 完全相同的时候冲突
                        if !secondary_depot_dir.recursive && !local_diff.is_empty() {
                            return Ok(());
                        }

                        // secondary_depot_dir + local_diff
                        // 与 primary_depot_file
                        // 都会映射到 primary_local_file
                        let mut depot_path_1_dir = Vec::new();
                        depot_path_1_dir.extend_from_slice(&secondary_depot_dir.dirs);
                        depot_path_1_dir.extend_from_slice(local_diff);
                        let depot_path_1 = DepotPath {
                            dirs: depot_path_1_dir,
                            file: conflict_file_example.clone(),
                        };
                        let depot_path_2 = DepotPath {
                            dirs: primary_depot_file.dirs.clone(),
                            file: primary_depot_file.file.clone(),
                        };
                        let local_path = LocalPath {
                            dirs: primary_local_file.dirs.clone(),
                            file: conflict_file_example.clone(),
                        };

                        return Err(Self::conflict_message(
                            &depot_path_1,
                            &depot_path_2,
                            &local_path,
                        ));
                    }
                } else {
                    // case 2.3. local 不包含，depot 不包含，不冲突
                    // case 2.4. local 不包含，depot 包含，不冲突
                    return Ok(());
                }
            }
            // case 3. primary 是目录，secondary 是文件
            (IncludeMapping::Folder(primary_mapping), IncludeMapping::File(secondary_mapping)) => {
                let primary_local_dir = &primary_mapping.local_folder;
                let primary_depot_dir = &primary_mapping.depot_folder;
                let secondary_local_file = &secondary_mapping.local_file;
                let secondary_depot_file = &secondary_mapping.depot_file;
                let local_common_prefix_end_index = Self::common_prefix_end_index(
                    &primary_local_dir.0,
                    &secondary_local_file.dirs.0,
                );
                let depot_common_prefix_end_index = Self::common_prefix_end_index(
                    &primary_depot_dir.dirs,
                    &secondary_depot_file.dirs,
                );
                // case 3.1. local 包含，depot 不包含，冲突
                if local_common_prefix_end_index == primary_local_dir.0.len()
                    && depot_common_prefix_end_index != primary_depot_dir.dirs.len()
                {
                    let local_diff = &secondary_local_file.dirs.0[local_common_prefix_end_index..];
                    // 非递归目录的情况下，当且仅当 local dir 完全相同的时候冲突
                    if !primary_depot_dir.recursive && !local_diff.is_empty() {
                        return Ok(());
                    }
                    // secondary_depot_file
                    // 与 primary_depot_dir + local_diff
                    // 都会映射到 secondary_local_file
                    // 检查后缀名，生成报错信息
                    let mut depot_path_1_dir = Vec::new();
                    depot_path_1_dir.extend_from_slice(&primary_depot_dir.dirs);
                    depot_path_1_dir.extend_from_slice(local_diff);
                    let depot_path_1 = DepotPath {
                        dirs: depot_path_1_dir,
                        file: conflict_file_example.clone(),
                    };
                    let depot_path_2 = DepotPath {
                        dirs: secondary_depot_file.dirs.clone(),
                        file: secondary_depot_file.file.clone(),
                    };
                    let local_path = LocalPath {
                        dirs: secondary_local_file.dirs.clone(),
                        file: conflict_file_example.clone(),
                    };

                    return Err(Self::conflict_message(
                        &depot_path_1,
                        &depot_path_2,
                        &local_path,
                    ));
                } else {
                    // case 3.2. 否则，不冲突
                    return Ok(());
                }
            }
            // case 4. 都是目录
            (
                IncludeMapping::Folder(primary_mapping),
                IncludeMapping::Folder(secondary_mapping),
            ) => {
                let primary_local_dir = &primary_mapping.local_folder;
                let secondary_local_dir = &secondary_mapping.local_folder;
                let primary_depot_dir = &primary_mapping.depot_folder;
                let secondary_depot_dir = &secondary_mapping.depot_folder;
                let local_common_prefix_end_index =
                    Self::common_prefix_end_index(&primary_local_dir.0, &secondary_local_dir.0);
                let depot_common_prefix_end_index = Self::common_prefix_end_index(
                    &primary_depot_dir.dirs,
                    &secondary_depot_dir.dirs,
                );

                // case 4.1. local 不相互包含的情况，一定不冲突
                if local_common_prefix_end_index
                    != primary_local_dir.0.len().min(secondary_local_dir.0.len())
                {
                    return Ok(());
                }
                // case 4.2. primary local 比 secondary local 长
                if primary_local_dir.0.len() > secondary_local_dir.0.len() {
                    // local 差异部分
                    let local_diff = &primary_local_dir.0[local_common_prefix_end_index..];
                    // case 4.2.1. primary depot 与 secondary depot 相互包含
                    if depot_common_prefix_end_index
                        == primary_depot_dir
                            .dirs
                            .len()
                            .min(secondary_depot_dir.dirs.len())
                    {
                        if primary_depot_dir.dirs.len() > secondary_depot_dir.dirs.len() {
                            let depot_diff =
                                &primary_depot_dir.dirs[depot_common_prefix_end_index..];
                            // case 4.2.1.1. primary depot 更长，但是差异部分没有比 local 的差异部分更长
                            if depot_diff.len() <= local_diff.len() {
                                let diff_common_prefix_end_index =
                                    Self::common_prefix_end_index(local_diff, depot_diff);
                                // case 4.2.1.1.1. 差异部分相互包含，不冲突
                                if diff_common_prefix_end_index == depot_diff.len() {
                                    return Ok(());
                                } else {
                                    // case 4.2.1.1.2. 差异部分不相互包含，
                                    // 且 secondary_depot 是递归目录时，存在冲突
                                    if !secondary_depot_dir.recursive {
                                        return Ok(());
                                    }
                                    let local_diff_diff =
                                        &local_diff[diff_common_prefix_end_index..];
                                    let depot_prefix_and_diff_prefix = &primary_depot_dir.dirs[0
                                        ..depot_common_prefix_end_index
                                            + diff_common_prefix_end_index];
                                    // depot_prefix_and_diff_prefix + local_diff_diff
                                    // 与 primary_depot_dir 都会映射到 primary_local_dir。
                                    let mut depot_path_1_dir = Vec::new();
                                    depot_path_1_dir
                                        .extend_from_slice(depot_prefix_and_diff_prefix);
                                    depot_path_1_dir.extend_from_slice(local_diff_diff);
                                    let depot_path_1 = DepotPath {
                                        dirs: depot_path_1_dir,
                                        file: conflict_file_example.clone(),
                                    };
                                    let depot_path_2 = DepotPath {
                                        dirs: primary_depot_dir.dirs.clone(),
                                        file: conflict_file_example.clone(),
                                    };
                                    let local_path = LocalPath {
                                        dirs: primary_local_dir.clone(),
                                        file: conflict_file_example.clone(),
                                    };

                                    return Err(Self::conflict_message(
                                        &depot_path_1,
                                        &depot_path_2,
                                        &local_path,
                                    ));
                                }
                            } else {
                                // case 4.2.1.2. primary depot 更长，且差异部分比 local 的差异部分更长，
                                // 且 secondary_depot 是递归目录时，存在冲突
                                if !secondary_depot_dir.recursive {
                                    return Ok(());
                                }

                                let diff_common_prefix_end_index =
                                    Self::common_prefix_end_index(local_diff, depot_diff);
                                let local_diff_diff = &local_diff[diff_common_prefix_end_index..];
                                let depot_prefix_and_diff_prefix = &primary_depot_dir.dirs[0
                                    ..depot_common_prefix_end_index + diff_common_prefix_end_index];
                                // depot_prefix_and_diff_prefix + local_diff_diff
                                // 与 primary_depot_dir 都会映射到 primary_local_dir。

                                let mut depot_path_1_dir = Vec::new();
                                depot_path_1_dir.extend_from_slice(depot_prefix_and_diff_prefix);
                                depot_path_1_dir.extend_from_slice(local_diff_diff);
                                let depot_path_1 = DepotPath {
                                    dirs: depot_path_1_dir,
                                    file: conflict_file_example.clone(),
                                };
                                let depot_path_2 = DepotPath {
                                    dirs: primary_depot_dir.dirs.clone(),
                                    file: conflict_file_example.clone(),
                                };
                                let local_path = LocalPath {
                                    dirs: primary_local_dir.clone(),
                                    file: conflict_file_example.clone(),
                                };

                                return Err(Self::conflict_message(
                                    &depot_path_1,
                                    &depot_path_2,
                                    &local_path,
                                ));
                            }
                        } else {
                            // case 4.2.1.3. depot 一样长，或者 secondary depot 更长，不冲突
                            return Ok(());
                        }
                    } else {
                        // case 4.2.2. primary depot 与 secondary depot 不相互包含，
                        // 且 secondary depot 是递归目录时，冲突
                        if !secondary_depot_dir.recursive {
                            return Ok(());
                        }
                        // secondary_depot_dir + local_diff
                        // 与 primary_depot_dir 都会映射到 primary_local_dir

                        let mut depot_path_1_dir = Vec::new();
                        depot_path_1_dir.extend_from_slice(&secondary_depot_dir.dirs);
                        depot_path_1_dir.extend_from_slice(local_diff);
                        let depot_path_1 = DepotPath {
                            dirs: depot_path_1_dir,
                            file: conflict_file_example.clone(),
                        };
                        let depot_path_2 = DepotPath {
                            dirs: primary_depot_dir.dirs.clone(),
                            file: conflict_file_example.clone(),
                        };
                        let local_path = LocalPath {
                            dirs: primary_local_dir.clone(),
                            file: conflict_file_example.clone(),
                        };

                        return Err(Self::conflict_message(
                            &depot_path_1,
                            &depot_path_2,
                            &local_path,
                        ));
                    }
                } else if primary_local_dir.0.len() < secondary_local_dir.0.len() {
                    // case 4.3. primary local 比 secondary local 短
                    // case 4.3.1. primary depot 与 secondary depot 相互包含
                    if depot_common_prefix_end_index
                        == primary_depot_dir
                            .dirs
                            .len()
                            .min(secondary_depot_dir.dirs.len())
                    {
                        // case 4.3.1.1. primary depot 更长，且 primary depot 是递归目录时，冲突
                        if primary_depot_dir.dirs.len() > secondary_depot_dir.dirs.len() {
                            if !primary_depot_dir.recursive {
                                return Ok(());
                            }

                            // local 差异部分
                            let local_diff =
                                &secondary_local_dir.0[local_common_prefix_end_index..];
                            // secondary_depot_dir + local_diff
                            // 与 primary_depot_dir + local_diff
                            // 都会映射到 secondary_local_dir

                            let mut depot_path_1_dir = Vec::new();
                            depot_path_1_dir.extend_from_slice(&secondary_depot_dir.dirs);
                            depot_path_1_dir.extend_from_slice(local_diff);
                            let depot_path_1 = DepotPath {
                                dirs: depot_path_1_dir,
                                file: conflict_file_example.clone(),
                            };
                            let mut depot_path_2_dir = Vec::new();
                            depot_path_2_dir.extend_from_slice(&primary_depot_dir.dirs);
                            depot_path_2_dir.extend_from_slice(local_diff);

                            let depot_path_2 = DepotPath {
                                dirs: depot_path_2_dir,
                                file: conflict_file_example.clone(),
                            };
                            let local_path = LocalPath {
                                dirs: secondary_local_dir.clone(),
                                file: conflict_file_example.clone(),
                            };

                            return Err(Self::conflict_message(
                                &depot_path_1,
                                &depot_path_2,
                                &local_path,
                            ));
                        } else {
                            // case 4.3.1.2. depot 一样长，或者 secondary depot 更长，不冲突
                            return Ok(());
                        }
                    } else {
                        // case 4.3.2. primary depot 与 secondary depot 不相互包含，
                        // 且 primary depot 是递归目录时，冲突
                        if !primary_depot_dir.recursive {
                            return Ok(());
                        }

                        let local_diff = &secondary_local_dir.0[local_common_prefix_end_index..];
                        // primary_depot_dir + local_diff
                        // 与 secondary_depot_dir
                        // 都会映射到 secondary_local_dir

                        let mut depot_path_1_dir = Vec::new();
                        depot_path_1_dir.extend_from_slice(&primary_depot_dir.dirs);
                        depot_path_1_dir.extend_from_slice(local_diff);
                        let depot_path_1 = DepotPath {
                            dirs: depot_path_1_dir,
                            file: conflict_file_example.clone(),
                        };
                        let depot_path_2 = DepotPath {
                            dirs: secondary_depot_dir.dirs.clone(),
                            file: conflict_file_example.clone(),
                        };
                        let local_path = LocalPath {
                            dirs: secondary_local_dir.clone(),
                            file: conflict_file_example.clone(),
                        };

                        return Err(Self::conflict_message(
                            &depot_path_1,
                            &depot_path_2,
                            &local_path,
                        ));
                    }
                } else {
                    // case 4.4. primary local 和 secondary local 一样长
                    // case 4.4.1. primary depot 包含在 secondary depot 内，不冲突
                    if depot_common_prefix_end_index == primary_depot_dir.dirs.len() {
                        return Ok(());
                    } else {
                        // case 4.4.2. primary depot 不包含在 secondary depot 内，冲突
                        // primary_depot_dir 与 secondary_depot_dir 都会映射到 secondary_local_dir
                        let depot_path_1 = DepotPath {
                            dirs: primary_depot_dir.dirs.clone(),
                            file: conflict_file_example.clone(),
                        };
                        let depot_path_2 = DepotPath {
                            dirs: secondary_depot_dir.dirs.clone(),
                            file: conflict_file_example.clone(),
                        };
                        let local_path = LocalPath {
                            dirs: secondary_local_dir.clone(),
                            file: conflict_file_example.clone(),
                        };

                        return Err(Self::conflict_message(
                            &depot_path_1,
                            &depot_path_2,
                            &local_path,
                        ));
                    }
                }
            }
        }
    }

    /// 求两个切片的公共前缀的结束索引，如果返回 0 则代表没有公共前缀
    pub fn common_prefix_end_index(slice_a: &[String], slice_b: &[String]) -> usize {
        for (i, (a, b)) in slice_a.iter().zip(slice_b.iter()).enumerate() {
            if a != b {
                return i;
            }
        }
        slice_a.len().min(slice_b.len())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_common_prefix_end_index() {
        let a = &["1".to_string(), "2".to_string()];
        let b = &["1".to_string(), "2".to_string()];
        assert_eq!(WorkspaceConfig::common_prefix_end_index(a, b), 2);
        let a = &["1".to_string(), "3".to_string()];
        let b = &["2".to_string(), "4".to_string()];
        assert_eq!(WorkspaceConfig::common_prefix_end_index(a, b), 0);
        let a = &["1".to_string()];
        let b = &["1".to_string(), "1".to_string()];
        assert_eq!(WorkspaceConfig::common_prefix_end_index(a, b), 1);
        let a = &["1".to_string(), "1".to_string()];
        let b = &["1".to_string()];
        assert_eq!(WorkspaceConfig::common_prefix_end_index(a, b), 1);
        let a = &[];
        let b = &["1".to_string(), "1".to_string()];
        assert_eq!(WorkspaceConfig::common_prefix_end_index(a, b), 0);
        let a = &["1".to_string(), "1".to_string()];
        let b = &[];
        assert_eq!(WorkspaceConfig::common_prefix_end_index(a, b), 0);
    }

    #[test]
    fn test_from_specification() {
        // case 0. 正常解析
        let root_dir = r#"/user"#;
        let mappings = r#"
            //a/b/c/... /user
            //a/b/c/d/... /user/d/ "#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        println!("case 0. 正常解析：{:?}", workspace_config);
        assert!(workspace_config.is_ok());
        let root_dir = r#"/root/workspace"#;
        let mappings = r#"
            //a/b/...~a /root/workspace/a/b
            //a/b/...~b /root/workspace/a/b
            //a/b/txt.a /root/workspace/a/b/txt.a"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        println!("case 0. 正常解析：{:?}", workspace_config);
        assert!(workspace_config.is_ok());
        let root_dir = r#"/root/workspace"#;
        let mappings = r#"
            //a/b/~a /root/workspace/a/b
            //a/b/~b /root/workspace/a/b
            //a/b/txt.a /root/workspace/a/b/txt.a"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        println!("case 0. 正常解析：{:?}", workspace_config);
        assert!(workspace_config.is_ok());

        let root_dir = r#"D:\temp"#;
        let mappings = r#"
            //a/b/c/... D:\temp
            //a/b/c/d/... D:\temp\d\ "#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        println!("case 0. 正常解析：{:?}", workspace_config);
        assert!(workspace_config.is_ok());
        // case 1. 语法错误
        // root dir 语法错误
        let root_dir = r#"C:\~"#;
        let mappings = r#"//a/b/c/... C:\User"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::SyntaxError(_)
        ));
        println!("case 1. 语法错误: {}", workspace_config_err);
        // workspace mapping 语法错误
        let root_dir = r#"C:\User"#;
        let mappings = r#"//a/b/c/~~~ C:\User"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::SyntaxError(_)
        ));
        println!("case 1. 语法错误: {}", workspace_config_err);
        // root dir 和 workspace mapping 均语法错误
        let root_dir = r#"C:\~"#;
        let mappings = r#"//a/b/c/~~~ C:\User"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::SyntaxError(_)
        ));
        println!("case 1. 语法错误: {}", workspace_config_err);
        // case 2. mapping 中的 local 不在 root dir 下
        let root_dir = r#"C:\User"#;
        let mappings = r#"//a/b/c/... D:\temp"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::MappingNotUnderRoot(_)
        ));
        println!(
            "case 2. mapping 中的 local 不在 root dir 下：{}",
            workspace_config_err
        );
        // case 3. mapping 冲突
        let root_dir = r#"D:\temp"#;
        let mappings = r#"
            //a/b/c/... D:\temp
            //a/b/c/d/... D:\temp\e\ "#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        println!("case 3. mapping 冲突：{}", workspace_config_err);
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::MappingConflictError(_)
        ));
        let root_dir = r#"D:\temp"#;
        let mappings = r#"
            //a/b/c/... D:\temp\a\b
            //a/b/d/... D:\temp\
        "#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        println!("case 3. mapping 冲突：{}", workspace_config_err);
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::MappingConflictError(_)
        ));
        let root_dir = r#"/root/workspace"#;
        let mappings = r#"
            //a/b/...~a /root/workspace/a/b
            //a/b/...~b /root/workspace/a/b
            //a/b/txt.b /root/workspace/a/b/txt.a"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        println!("case 3. mapping 冲突：{}", workspace_config_err);
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::MappingConflictError(_)
        ));
        let root_dir = r#"/root/workspace"#;
        let mappings = r#"
            //a/b/~a /root/workspace/a/b
            //a/b/~b /root/workspace/a/b
            //a/b/txt.b /root/workspace/a/b/txt.a"#;
        let workspace_config = WorkspaceConfig::from_specification(root_dir, mappings);
        assert!(workspace_config.is_err());
        let workspace_config_err = workspace_config.err().unwrap();
        println!("case 3. mapping 冲突：{}", workspace_config_err);
        assert!(matches!(
            workspace_config_err,
            WorkspaceError::MappingConflictError(_)
        ));
    }
}
