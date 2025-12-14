use std::collections::{HashMap, HashSet};

use super::{construct_tree_from_changelist, FileTree, FileTreeResult};

/// Branch 级别的 DepotTree 状态。
///
/// - 维护该分支下的文件数量缓存；
/// - 维护基于 changelist + 路径通配构建出的文件树缓存。
#[derive(Debug, Default, Clone)]
pub struct BranchDepotState {
    /// 当前分支下文件总数缓存（例如用于 UI 展示、快速统计）。
    pub file_count: usize,

    /// 文件树缓存：key = (changelist_id, depot_wildcard)
    file_tree_cache: HashMap<(i64, String), FileTree>,
}

impl BranchDepotState {
    /// 缓存某个 changelist + 路径通配下的文件树。
    pub fn cache_file_tree(
        &mut self,
        changelist_id: i64,
        depot_wildcard: &str,
        tree: FileTree,
    ) {
        self.file_tree_cache
            .insert((changelist_id, depot_wildcard.to_string()), tree);
    }

    /// 获取某个 changelist + 路径通配下缓存的文件树。
    pub fn get_cached_file_tree(
        &self,
        changelist_id: i64,
        depot_wildcard: &str,
    ) -> Option<&FileTree> {
        self.file_tree_cache
            .get(&(changelist_id, depot_wildcard.to_string()))
    }

    /// 清除某个 changelist 的所有文件树缓存。
    pub fn clear_file_tree_cache_for_changelist(&mut self, changelist_id: i64) {
        self.file_tree_cache
            .retain(|(cl_id, _), _| *cl_id != changelist_id);
    }

    /// 清空整个文件树缓存。
    pub fn clear_all_file_tree_cache(&mut self) {
        self.file_tree_cache.clear();
    }
}

/// DepotTree：在内存中维护所有分支的 Depot 视图状态。
///
/// 该结构本身不涉及并发控制，也不直接访问数据库，仅作为
/// 上层应用（如 crv-hive）在内存中的运行时缓存与锁管理对象。
#[derive(Debug, Default, Clone)]
pub struct DepotTree {
    branches: HashMap<String, BranchDepotState>,
    /// 全局文件锁集合，key = (branch_id, file_id)
    locked_files: HashSet<(String, String)>,
}

impl DepotTree {
    /// 创建一个新的空 DepotTree。
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取指定分支的状态（如果不存在则自动创建）。
    fn branch_mut(&mut self, branch_id: &str) -> &mut BranchDepotState {
        self.branches
            .entry(branch_id.to_string())
            .or_insert_with(BranchDepotState::default)
    }

    /// 获取指定分支的状态（只读）。
    pub fn branch(&self, branch_id: &str) -> Option<&BranchDepotState> {
        self.branches.get(branch_id)
    }

    /// 设置指定分支的文件总数缓存。
    pub fn set_file_count(&mut self, branch_id: &str, count: usize) {
        let state = self.branch_mut(branch_id);
        state.file_count = count;
    }

    /// 获取指定分支的文件总数缓存。
    ///
    /// 如该分支尚未出现，则返回 0。
    pub fn get_file_count(&self, branch_id: &str) -> usize {
        self.branches
            .get(branch_id)
            .map(|s| s.file_count)
            .unwrap_or(0)
    }

    /// 为指定分支的一组文件尝试加锁。
    ///
    /// - `locked`：本次成功加锁的文件 ID 列表；
    /// - `conflicted`：已经被其他操作锁定、无法加锁的文件 ID 列表。
    pub fn try_lock_files<I>(
        &mut self,
        branch_id: &str,
        file_ids: I,
    ) -> (Vec<String>, Vec<String>)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let branch = branch_id.to_string();
        let mut unique_ids = HashSet::new();
        for fid in file_ids {
            unique_ids.insert(fid.as_ref().to_string());
        }

        // 先检查是否存在已被锁定的 (branch, file) 组合
        let conflicted: Vec<String> = unique_ids
            .iter()
            .filter(|fid| self.locked_files.contains(&(branch.clone(), (*fid).clone())))
            .cloned()
            .collect();

        if !conflicted.is_empty() {
            // 全部失败，不加任何新锁
            return (Vec::new(), conflicted);
        }

        // 所有文件均可加锁，一次性加锁
        for fid in &unique_ids {
            self.locked_files.insert((branch.clone(), fid.clone()));
        }

        let locked = unique_ids.into_iter().collect();
        (locked, Vec::new())
    }

    /// 释放指定分支下一组文件的锁。
    pub fn unlock_files<I>(&mut self, branch_id: &str, file_ids: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let branch = branch_id.to_string();
        for fid in file_ids {
            let key = (branch.clone(), fid.as_ref().to_string());
            self.locked_files.remove(&key);
        }
    }

    /// 查询某个文件在指定分支下当前是否已被锁定。
    pub fn is_locked(&self, branch_id: &str, file_id: &str) -> bool {
        self.locked_files
            .contains(&(branch_id.to_string(), file_id.to_string()))
    }

    /// 为指定分支缓存某个 changelist + 路径通配下的文件树。
    pub fn cache_file_tree(
        &mut self,
        branch_id: &str,
        changelist_id: i64,
        depot_wildcard: &str,
        tree: FileTree,
    ) {
        let state = self.branch_mut(branch_id);
        state.cache_file_tree(changelist_id, depot_wildcard, tree);
    }

    /// 获取指定分支下某个 changelist + 路径通配的文件树缓存。
    pub fn get_cached_file_tree(
        &self,
        branch_id: &str,
        changelist_id: i64,
        depot_wildcard: &str,
    ) -> Option<&FileTree> {
        self.branches
            .get(branch_id)
            .and_then(|s| s.get_cached_file_tree(changelist_id, depot_wildcard))
    }

    /// 获取（或计算并缓存）指定分支、changelist 与路径通配符下的文件树。
    /// TODO: 这里应该存在一个优化，在某个 changelist 后的 depot tree 可以通过之前 changelist 的缓存的
    /// depot tree 构建。在某个 changelist 前的也可以通过对缓存的 depot tree 进行前向回溯操作进行构建从而
    /// 加快文件树的构建。
    ///
    /// - 若缓存中已存在，则返回其克隆副本；
    /// - 若不存在，则调用 `construct_tree_from_changelist` 计算出文件树并写入缓存后返回。
    pub fn get_or_construct_file_tree<GB, GC, GF, GR>(
        &mut self,
        branch_id: &str,
        depot_wildcard: &str,
        changelist_id: i64,
        mut get_branch: GB,
        mut get_changelist: GC,
        mut get_file: GF,
        mut get_file_revision: GR,
    ) -> FileTreeResult<FileTree>
    where
        GB: FnMut(&str) -> Result<Option<crate::metadata::BranchDoc>, String>,
        GC: FnMut(i64) -> Result<Option<crate::metadata::ChangelistDoc>, String>,
        GF: FnMut(&str) -> Result<Option<crate::metadata::FileDoc>, String>,
        GR: FnMut(&str) -> Result<Option<crate::metadata::FileRevisionDoc>, String>,
    {
        // 1. 先尝试从已有缓存中获取
        if let Some(state) = self.branches.get(branch_id) {
            if let Some(tree) = state.get_cached_file_tree(changelist_id, depot_wildcard) {
                return Ok(tree.clone());
            }
        }

        // 2. 缓存中没有，则计算新的文件树
        let tree = construct_tree_from_changelist(
            branch_id,
            depot_wildcard,
            changelist_id,
            &mut get_branch,
            &mut get_changelist,
            &mut get_file,
            &mut get_file_revision,
        )?;

        // 3. 写入缓存并返回克隆副本
        let result = tree.clone();
        let state = self.branch_mut(branch_id);
        state.cache_file_tree(changelist_id, depot_wildcard, tree);
        Ok(result)
    }

    /// 清除指定分支某个 changelist 的文件树缓存。
    pub fn clear_file_tree_cache_for_changelist(&mut self, branch_id: &str, changelist_id: i64) {
        if let Some(state) = self.branches.get_mut(branch_id) {
            state.clear_file_tree_cache_for_changelist(changelist_id);
        }
    }

    /// 清空指定分支的所有文件树缓存。
    pub fn clear_all_file_tree_cache(&mut self, branch_id: &str) {
        if let Some(state) = self.branches.get_mut(branch_id) {
            state.clear_all_file_tree_cache();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_count_cache() {
        let mut depot = DepotTree::new();
        assert_eq!(depot.get_file_count("branch_main"), 0);

        depot.set_file_count("branch_main", 42);
        assert_eq!(depot.get_file_count("branch_main"), 42);
    }

    #[test]
    fn test_file_locking_basic() {
        let mut depot = DepotTree::new();

        let (locked, conflicted) =
            depot.try_lock_files("branch_main", ["f1", "f2", "f1"].as_ref());
        // f1、f2 应该被成功加锁，重复的 f1 不影响结果
        assert_eq!(locked.len(), 2);
        assert!(locked.contains(&"f1".to_string()));
        assert!(locked.contains(&"f2".to_string()));
        assert!(conflicted.is_empty());

        // 再次尝试锁 f1，应当冲突
        let (_locked2, conflicted2) = depot.try_lock_files("branch_main", ["f1"].as_ref());
        assert_eq!(conflicted2, vec!["f1".to_string()]);
        assert!(depot.is_locked("branch_main", "f1"));

        // 解锁 f1 后应当可以再次加锁
        depot.unlock_files("branch_main", ["f1"].as_ref());
        assert!(!depot.is_locked("branch_main", "f1"));
        let (locked3, conflicted3) = depot.try_lock_files("branch_main", ["f1"].as_ref());
        assert_eq!(locked3, vec!["f1".to_string()]);
        assert!(conflicted3.is_empty());
    }

    #[test]
    fn test_branch_isolation_for_locks() {
        let mut depot = DepotTree::new();

        let (_locked_a, conflicted_a) = depot.try_lock_files("branch_a", ["f1"].as_ref());
        assert!(conflicted_a.is_empty());

        // 在另一分支上锁同一个 file_id 应该不冲突
        let (_locked_b, conflicted_b) = depot.try_lock_files("branch_b", ["f1"].as_ref());
        assert!(conflicted_b.is_empty());

        assert!(depot.is_locked("branch_a", "f1"));
        assert!(depot.is_locked("branch_b", "f1"));
    }

    #[test]
    fn test_file_tree_cache_per_branch_and_changelist() {
        let mut depot = DepotTree::new();

        let tree_a = FileTree { nodes: vec![] };
        let tree_b = FileTree { nodes: vec![] };

        depot.cache_file_tree("branch_main", 100, "//src/module/...", tree_a.clone());
        depot.cache_file_tree("branch_main", 200, "//src/module/...", tree_b.clone());

        assert!(depot
            .get_cached_file_tree("branch_main", 100, "//src/module/...")
            .is_some());
        assert!(depot
            .get_cached_file_tree("branch_main", 200, "//src/module/...")
            .is_some());
        assert!(depot
            .get_cached_file_tree("branch_main", 300, "//src/module/...")
            .is_none());

        depot.clear_file_tree_cache_for_changelist("branch_main", 100);
        assert!(depot
            .get_cached_file_tree("branch_main", 100, "//src/module/...")
            .is_none());
        assert!(depot
            .get_cached_file_tree("branch_main", 200, "//src/module/...")
            .is_some());

        depot.clear_all_file_tree_cache("branch_main");
        assert!(depot
            .get_cached_file_tree("branch_main", 200, "//src/module/...")
            .is_none());
    }
}


