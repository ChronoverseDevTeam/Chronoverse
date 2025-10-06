#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use super::super::block_manager::BlockManager;

    fn setup_test_dirs() -> (PathBuf, PathBuf) {
        let workspace_root = PathBuf::from("test_workspace");
        let depot_root = PathBuf::from("test_depot");

        // 清理并创建测试目录
        if workspace_root.exists() {
            fs::remove_dir_all(&workspace_root).unwrap();
        }
        if depot_root.exists() {
            fs::remove_dir_all(&depot_root).unwrap();
        }
        fs::create_dir_all(&workspace_root).unwrap();
        fs::create_dir_all(&depot_root).unwrap();

        (workspace_root, depot_root)
    }

    fn create_test_files(depot_root: &PathBuf) -> Vec<(String, String)> {
        let test_files = vec![
            ("file1.txt", "Hello, this is file 1 content!"),
            ("file2.txt", "This is the second file with some different content."),
            ("file3.txt", "And here's the third file with unique text."),
        ];

        for (filename, content) in &test_files {
            let file_path = depot_root.join(filename);
            fs::write(&file_path, content).unwrap();
        }

        test_files.into_iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn test_file_operations() {
        // 1. 设置测试环境
        let (workspace_root, depot_root) = setup_test_dirs();
        let test_files = create_test_files(&depot_root);
        
        // 2. 创建 BlockManager 实例
        let mut block_manager = BlockManager::new(&workspace_root, &depot_root).unwrap();

        // 3. 提交文件到 depot
        let mut file_hashes = Vec::new();
        for (filename, _) in &test_files {
            let file_path = depot_root.join(filename);
            let hashes = block_manager.create_blocks_at_path(&file_path).unwrap();
            file_hashes.push((filename.clone(), hashes));
        }

        // 4. 清理 workspace 目录
        fs::remove_dir_all(&workspace_root).unwrap();
        fs::create_dir_all(&workspace_root).unwrap();

        // 5. 从 depot 拉取文件到 workspace
        for (filename, hashes) in &file_hashes {
            let content = block_manager.get_block_content_by_hashs(hashes.clone()).unwrap();
            let file_path = workspace_root.join(filename);
            fs::write(&file_path, content).unwrap();
        }

        // 6. 验证文件内容
        for (filename, original_content) in &test_files {
            let file_path = workspace_root.join(filename);
            let content = fs::read_to_string(&file_path).unwrap();
            assert_eq!(content, *original_content);
        }

        // 7. 修改两个文件并重新提交
        let modifications = vec![
            ("file1.txt", "Modified content for file 1!"),
            ("file2.txt", "Updated content of file 2 with changes."),
        ];

        for (filename, new_content) in modifications {
            let file_path = workspace_root.join(filename);
            fs::write(&file_path, new_content).unwrap();
            
            // 重新提交修改后的文件
            let new_hashes = block_manager.create_blocks_at_path(&file_path).unwrap();
            
            // 更新文件哈希
            if let Some(entry) = file_hashes.iter_mut().find(|(name, _)| name == filename) {
                entry.1 = new_hashes;
            }
        }

        // 8. 清理并重新拉取所有文件
        fs::remove_dir_all(&workspace_root).unwrap();
        fs::create_dir_all(&workspace_root).unwrap();

        // 9. 重新拉取并验证修改后的内容
        for (filename, hashes) in &file_hashes {
            let content = block_manager.get_block_content_by_hashs(hashes.clone()).unwrap();
            let file_path = workspace_root.join(filename);
            fs::write(&file_path, content).unwrap();
        }

        // 10. 验证最终文件内容
        for (filename, new_content) in modifications {
            let file_path = workspace_root.join(filename);
            let content = fs::read_to_string(&file_path).unwrap();
            assert_eq!(content, new_content);
        }

        // 验证未修改的文件保持原样
        let file3_path = workspace_root.join("file3.txt");
        let content = fs::read_to_string(&file3_path).unwrap();
        assert_eq!(content, test_files[2].1);

        // 清理测试目录
        fs::remove_dir_all(&workspace_root).unwrap();
        fs::remove_dir_all(&depot_root).unwrap();
    }
}