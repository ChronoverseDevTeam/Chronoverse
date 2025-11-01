use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use chrono::Utc;
use crv_core::metadata::changelist::Changelist;
use crv_core::metadata::file::MetaFile;
use crv_core::metadata::file_revision::MetaFileRevision;
use crv_core::storage::ChunkingOptions;
use tonic::transport::Channel;
use crate::pb::edge_daemon_service_client::EdgeDaemonServiceClient;
use crate::pb::{
    BonjourReq, BonjourRsp,
    CreateWorkspaceReq, CreateWorkspaceRsp,
    GetLatestReq,
    CheckoutReq,
    SubmitReq,
    HiveConnectReq, HiveConnectRsp,
    HiveLoginReq, HiveLoginRsp,
    HiveRegisterReq, HiveRegisterRsp,
    HiveListWorkspacesReq, HiveListWorkspacesRsp,
};

/// Local workspace file state - tracks what's checked out locally
#[derive(Debug)]
pub struct LocalFileState {
    /// Current revision checked out in workspace
    pub current_revision: u64,
    /// Depot path (server-side canonical path)
    pub depot_path: String,
    /// Local filesystem path
    pub local_path: PathBuf,
    /// Whether file has been modified since checkout
    pub is_modified: bool,
}

/// CRV Client - 支持本地模拟和 gRPC 两种模式
/// 
/// **本地模拟模式** (`use_local_simulation = true`):
/// - 完整模拟客户端-服务器交互
/// - 使用 metadata 类型（MetaFile, MetaFileRevision, Changelist）
/// - 用于测试和开发
/// 
/// **gRPC 模式** (`use_local_simulation = false`):
/// - 连接真实的 crv-edge 守护进程
/// - 通过 gRPC 调用服务器接口
/// - 服务器当前返回空回包（占位符实现）
pub struct CrvClient {
    /// 是否使用本地模拟（true=本地模拟，false=gRPC）
    use_local_simulation: bool,
    
    // === gRPC 客户端（仅在 gRPC 模式下使用）===
    grpc_client: Option<EdgeDaemonServiceClient<Channel>>,
    
    // === 本地模拟字段（仅在本地模拟模式下使用）===
    /// Local workspace root directory
    workspace_root: PathBuf,
    /// Simulated server depot root directory
    server_depot_root: PathBuf,
    /// Server block storage root (content-addressed storage)
    server_block_store: PathBuf,
    /// Files currently checked out in workspace (depot_path -> LocalFileState)
    local_files: HashMap<String, LocalFileState>,
    /// Server file metadata using MetaFile (depot_path -> MetaFile)
    server_files: HashMap<String, MetaFile>,
    /// Changelists on server (changelist_id -> Changelist)
    changelists: HashMap<u64, Changelist>,
    /// Next changelist ID to allocate
    next_changelist_id: u64,
    /// Chunking options for file block splitting
    chunking_options: ChunkingOptions,
}

impl CrvClient {
    /// 创建本地模拟模式的客户端
    /// 
    /// # Arguments
    /// * `workspace_root` - 本地工作空间根目录
    /// * `server_depot_root` - 模拟服务器仓库根目录
    /// 
    /// # Example
    /// ```no_run
    /// use crv_cli::client::CrvClient;
    /// let client = CrvClient::new("./workspace", "./server").unwrap();
    /// ```
    pub fn new<P: AsRef<Path>, Q: AsRef<Path>>(
        workspace_root: P,
        server_depot_root: Q,
    ) -> io::Result<Self> {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let server_depot_root = server_depot_root.as_ref().to_path_buf();
        let server_block_store = server_depot_root.join(".blocks");

        // Create directories
        fs::create_dir_all(&workspace_root)?;
        fs::create_dir_all(&server_depot_root)?;
        fs::create_dir_all(&server_block_store)?;

        Ok(Self {
            use_local_simulation: true,
            grpc_client: None,
            workspace_root,
            server_depot_root,
            server_block_store,
            local_files: HashMap::new(),
            server_files: HashMap::new(),
            changelists: HashMap::new(),
            next_changelist_id: 1,
            chunking_options: ChunkingOptions {
                fixed_block_size: 4 * 1024 * 1024,  // 4MB for large files
                small_file_threshold: 4 * 1024 * 1024, // 4MB threshold
                cdc_window_size: 48,
                cdc_min_size: 8 * 1024,              // 8KB
                cdc_avg_size: 32 * 1024,             // 32KB
                cdc_max_size: 64 * 1024,             // 64KB
            },
        })
    }

    /// 创建 gRPC 模式的客户端（连接真实服务器）
    /// 
    /// # Arguments
    /// * `server_addr` - 服务器地址（例如: "http://127.0.0.1:34562"）
    /// 
    /// # Example
    /// ```no_run
    /// use crv_cli::client::CrvClient;
    /// # async {
    /// let client = CrvClient::new_grpc("http://127.0.0.1:34562").await.unwrap();
    /// # };
    /// ```
    pub async fn new_grpc(server_addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let channel = Channel::from_shared(server_addr.to_string())?
            .connect()
            .await?;
        
        let grpc_client = EdgeDaemonServiceClient::new(channel);
        
        Ok(Self {
            use_local_simulation: false,
            grpc_client: Some(grpc_client),
            // 本地模拟字段使用默认值（gRPC 模式下不使用）
            workspace_root: PathBuf::new(),
            server_depot_root: PathBuf::new(),
            server_block_store: PathBuf::new(),
            local_files: HashMap::new(),
            server_files: HashMap::new(),
            changelists: HashMap::new(),
            next_changelist_id: 1,
            chunking_options: ChunkingOptions::default(),
        })
    }

    /// 启用本地模拟模式
    /// 
    /// 在已有的 gRPC 客户端基础上启用本地模拟功能。
    /// 此时会同时发送 gRPC 请求（打印回包）和执行本地模拟逻辑。
    pub fn enable_local_simulation<P: AsRef<Path>, Q: AsRef<Path>>(
        &mut self,
        workspace_root: P,
        server_depot_root: Q,
    ) -> io::Result<()> {
        let workspace_root = workspace_root.as_ref().to_path_buf();
        let server_depot_root = server_depot_root.as_ref().to_path_buf();
        let server_block_store = server_depot_root.join(".blocks");

        // Create directories
        fs::create_dir_all(&workspace_root)?;
        fs::create_dir_all(&server_depot_root)?;
        fs::create_dir_all(&server_block_store)?;

        self.use_local_simulation = true;
        self.workspace_root = workspace_root;
        self.server_depot_root = server_depot_root;
        self.server_block_store = server_block_store;
        self.chunking_options = ChunkingOptions {
            fixed_block_size: 4 * 1024 * 1024,
            small_file_threshold: 4 * 1024 * 1024,
            cdc_window_size: 48,
            cdc_min_size: 8 * 1024,
            cdc_avg_size: 32 * 1024,
            cdc_max_size: 64 * 1024,
        };

        Ok(())
    }

    /// 创建工作空间
    /// 
    /// 总是发送 gRPC 请求并打印回包。
    /// 如果启用本地模拟，则返回本地模拟结果；否则返回服务器响应。
    pub async fn create_workspace(&mut self) -> Result<CreateWorkspaceRsp, Box<dyn std::error::Error>> {
        // 总是发送 gRPC 请求
        let request = tonic::Request::new(CreateWorkspaceReq {});
        let grpc_response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .create_workspace(request)
            .await?;
        
        let grpc_rsp = grpc_response.into_inner();
        println!("📦 gRPC 回包: success={}, message={}, path={}", 
            grpc_rsp.success, grpc_rsp.message, grpc_rsp.workspace_path);
        
        if self.use_local_simulation {
            // 本地模拟模式：初始化服务器数据并返回本地结果
            self.init_server_with_sample_data()?;
            Ok(CreateWorkspaceRsp {
                success: true,
                message: "本地模拟工作空间已创建".to_string(),
                workspace_path: self.workspace_root.to_string_lossy().to_string(),
            })
        } else {
            // 纯 gRPC 模式：返回服务器响应
            Ok(grpc_rsp)
        }
    }

    /// 发送问候消息到服务器
    /// 
    /// 总是发送 gRPC 请求并打印回包。
    /// 如果启用本地模拟，则返回本地模拟结果；否则返回服务器响应。
    pub async fn bonjour(&mut self) -> Result<BonjourRsp, Box<dyn std::error::Error>> {
        // 总是发送 gRPC 请求
        let request = tonic::Request::new(BonjourReq {});
        let grpc_response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .bonjour(request)
            .await?;
        
        let grpc_rsp = grpc_response.into_inner();
        println!("📦 gRPC 回包: version={}, api_level={}, platform={}", 
            grpc_rsp.daemon_version, grpc_rsp.api_level, grpc_rsp.platform);
        
        if self.use_local_simulation {
            // 本地模拟模式：返回模拟响应
            Ok(BonjourRsp {
                daemon_version: "0.1.0-local-sim".to_string(),
                api_level: 1,
                platform: "simulation".to_string(),
                os: std::env::consts::OS.to_string(),
                architecture: std::env::consts::ARCH.to_string(),
            })
        } else {
            // 纯 gRPC 模式：返回服务器响应
            Ok(grpc_rsp)
        }
    }

    /// Initialize server with sample files and versions
    pub fn init_server_with_sample_data(&mut self) -> io::Result<()> {
        println!("📦 Initializing server with sample data...");

        // Create sample file 1 with 3 versions
        self.create_server_file_with_versions(
            "file1.txt",
            vec![
                "Version 1 of file1\nInitial content",
                "Version 2 of file1\nAdded more content",
                "Version 3 of file1\nFinal version with updates",
            ],
        )?;

        // Create sample file 2 with 2 versions
        self.create_server_file_with_versions(
            "file2.txt",
            vec![
                "Version 1 of file2\nBasic content",
                "Version 2 of file2\nUpdated content",
            ],
        )?;

        // Create sample file in subdirectory with 4 versions
        self.create_server_file_with_versions(
            "docs/readme.md",
            vec![
                "# README v1\nInitial documentation",
                "# README v2\nAdded installation guide",
                "# README v3\nAdded usage examples",
                "# README v4\nFinal documentation with all sections",
            ],
        )?;

        println!("✅ Server initialized with {} files", self.server_files.len());
        Ok(())
    }

    /// Helper: Create a server file with multiple versions
    /// 
    /// This simulates the server having pre-existing file history.
    /// Each version creates a MetaFileRevision and associated Changelist.
    fn create_server_file_with_versions(
        &mut self,
        depot_path: &str,
        versions: Vec<&str>,
    ) -> io::Result<()> {
        let mut revisions = Vec::new();

        for (idx, content) in versions.iter().enumerate() {
            let revision = (idx + 1) as u64;
            let changelist_id = self.next_changelist_id;
            self.next_changelist_id += 1;

            // Create temporary file with content
            let temp_file = self.server_depot_root.join(format!("temp_{}", depot_path.replace('/', "_")));
            if let Some(parent) = temp_file.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&temp_file, content)?;

            // Create MetaFileRevision from file (chunks and stores blocks)
            let file_revision = MetaFileRevision::from_source_file(
                depot_path.to_string(),
                revision,
                changelist_id,
                &temp_file,
                &self.server_block_store,
                &self.chunking_options,
            )?;

            // Create Changelist for this submission
            let changelist = Changelist {
                id: changelist_id,
                description: format!("Auto-generated changelist for {} v{}", depot_path, revision),
                created_at: Utc::now(),
                submitted_at: Some(Utc::now()),
                owner: "system".to_string(),
                files: vec![file_revision.clone()],
            };
            self.changelists.insert(changelist_id, changelist);

            revisions.push(file_revision);

            // Clean up temp file
            let _ = fs::remove_file(&temp_file);
        }

        // Create MetaFile to track all revisions of this file on server
        let meta_file = MetaFile {
            locked_by: String::new(), // Initially unlocked
            depot_path: depot_path.to_string(),
            revisions,
        };
        
        self.server_files.insert(depot_path.to_string(), meta_file);

        println!("  ✓ Created {} with {} versions", depot_path, versions.len());
        Ok(())
    }

    /// Get latest files from server
    /// 
    /// 总是发送 gRPC 请求并打印回包。
    /// 如果启用本地模拟，则返回本地模拟结果；否则返回服务器响应。
    pub async fn get_latest(&mut self) -> Result<Vec<String>, String> {
        // 总是发送 gRPC 请求
        let request = tonic::Request::new(GetLatestReq {});
        let grpc_response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .get_latest(request)
            .await
            .map_err(|e| format!("gRPC error: {}", e))?;
        
        let grpc_rsp = grpc_response.into_inner();
        println!("📦 gRPC 回包: success={}, files={:?}", grpc_rsp.success, grpc_rsp.file_paths);
        
        if self.use_local_simulation {
            // 本地模拟模式：返回模拟服务器的文件列表
            let file_list: Vec<String> = self.server_files.keys().cloned().collect();
            Ok(file_list)
        } else {
            // 纯 gRPC 模式：返回服务器响应
            if grpc_rsp.success {
                Ok(grpc_rsp.file_paths)
            } else {
                Err(grpc_rsp.message)
            }
        }
    }

    /// Checkout a file from server (latest version by default)
    /// 
    /// 总是发送 gRPC 请求并打印回包。
    /// 如果启用本地模拟，则返回本地模拟结果；否则返回服务器响应。
    pub async fn checkout(&mut self, depot_path: &str) -> Result<String, String> {
        // 总是发送 gRPC 请求
        let request = tonic::Request::new(CheckoutReq {
            relative_path: depot_path.to_string(),
        });
        let grpc_response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .checkout(request)
            .await
            .map_err(|e| format!("gRPC error: {}", e))?;
        
        let grpc_rsp = grpc_response.into_inner();
        println!("📦 gRPC 回包: success={}, message={}", grpc_rsp.success, grpc_rsp.message);
        
        if self.use_local_simulation {
            // 本地模拟模式：执行本地检出
            self.checkout_revision_local(depot_path, None)
        } else {
            // 纯 gRPC 模式：返回服务器响应
            if grpc_rsp.success {
                Ok(grpc_rsp.message)
            } else {
                Err(grpc_rsp.message)
            }
        }
    }

    /// Checkout a specific revision of a file (仅本地模拟模式)
    /// 
    /// Simulates: client requests file from server, server sends blocks,
    /// client reconstructs file in workspace
    fn checkout_revision_local(&mut self, depot_path: &str, revision: Option<u64>) -> Result<String, String> {
        // Get MetaFile from server
        let meta_file = self.server_files.get(depot_path)
            .ok_or_else(|| format!("File not found on server: {}", depot_path))?;

        // Determine which revision to checkout (default to latest)
        let target_revision = revision.unwrap_or_else(|| {
            meta_file.revisions.last()
                .map(|r| r.revision)
                .unwrap_or(1)
        });

        // Find the MetaFileRevision
        let file_revision = meta_file.revisions.iter()
            .find(|r| r.revision == target_revision)
            .ok_or_else(|| format!("Revision {} not found for {}", target_revision, depot_path))?;

        // Restore file to workspace from blocks
        let local_path = self.workspace_root.join(depot_path);
        file_revision.restore_to_path(&self.server_block_store, &local_path)
            .map_err(|e| format!("Failed to restore file: {}", e))?;

        // Update local workspace state
        self.local_files.insert(
            depot_path.to_string(),
            LocalFileState {
                current_revision: target_revision,
                depot_path: depot_path.to_string(),
                local_path: local_path.clone(),
                is_modified: false,
            },
        );

        Ok(format!("Checked out {} revision {} to {:?}", depot_path, target_revision, local_path))
    }

    /// Submit local changes to server (creates new revision)
    /// 
    /// 总是发送 gRPC 请求并打印回包。
    /// 如果启用本地模拟，则返回本地模拟结果；否则返回服务器响应。
    pub async fn submit(&mut self, depot_path: &str, description: String) -> Result<String, String> {
        // 总是发送 gRPC 请求
        let request = tonic::Request::new(SubmitReq {
            changelist_id: 1, // TODO: 支持指定 changelist_id
            description: description.clone(),
        });
        let grpc_response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .submit(request)
            .await
            .map_err(|e| format!("gRPC error: {}", e))?;
        
        let grpc_rsp = grpc_response.into_inner();
        println!("📦 gRPC 回包: success={}, message={}", grpc_rsp.success, grpc_rsp.message);
        
        if self.use_local_simulation {
            // 本地模拟模式：执行本地提交
            self.submit_local(depot_path, description)
        } else {
            // 纯 gRPC 模式：返回服务器响应
            if grpc_rsp.success {
                Ok(grpc_rsp.message)
            } else {
                Err(grpc_rsp.message)
            }
        }
    }

    /// Submit local changes (仅本地模拟模式)
    /// 
    /// Simulates: client chunks modified file, sends blocks to server,
    /// server creates new MetaFileRevision and Changelist
    fn submit_local(&mut self, depot_path: &str, description: String) -> Result<String, String> {
        // Check if file exists locally
        let local_state = self.local_files.get(depot_path)
            .ok_or_else(|| format!("File not checked out: {}", depot_path))?;

        // Check if file exists on disk
        if !local_state.local_path.exists() {
            return Err(format!("Local file not found: {:?}", local_state.local_path));
        }

        // Determine next revision number
        let next_revision = self.server_files.get(depot_path)
            .and_then(|mf| mf.revisions.last())
            .map(|r| r.revision + 1)
            .unwrap_or(1);

        // Create new changelist
        let changelist_id = self.next_changelist_id;
        self.next_changelist_id += 1;

        // Create new MetaFileRevision from local file (chunks and uploads blocks)
        let file_revision = MetaFileRevision::from_source_file(
            depot_path.to_string(),
            next_revision,
            changelist_id,
            &local_state.local_path,
            &self.server_block_store,
            &self.chunking_options,
        ).map_err(|e| format!("Failed to create revision: {}", e))?;

        // Create Changelist on server
        let changelist = Changelist {
            id: changelist_id,
            description: description.clone(),
            created_at: Utc::now(),
            submitted_at: Some(Utc::now()),
            owner: "user".to_string(),
            files: vec![file_revision.clone()],
        };
        self.changelists.insert(changelist_id, changelist);

        // Update server MetaFile with new revision
        if let Some(meta_file) = self.server_files.get_mut(depot_path) {
            meta_file.revisions.push(file_revision);
        } else {
            // File doesn't exist on server yet, create new MetaFile
            let meta_file = MetaFile {
                locked_by: String::new(),
                depot_path: depot_path.to_string(),
                revisions: vec![file_revision],
            };
            self.server_files.insert(depot_path.to_string(), meta_file);
        }

        // Update local workspace state
        if let Some(local) = self.local_files.get_mut(depot_path) {
            local.current_revision = next_revision;
            local.is_modified = false;
        }

        Ok(format!("Submitted {} as revision {} (changelist {})", depot_path, next_revision, changelist_id))
    }

    /// Change local file to a different revision (仅本地模拟模式支持)
    /// 
    /// 注意：gRPC 模式下此功能需要服务器支持特定版本检出
    pub fn change_revision(&mut self, depot_path: &str, target_revision: u64) -> Result<String, String> {
        if self.use_local_simulation {
            self.checkout_revision_local(depot_path, Some(target_revision))
        } else {
            Err("change_revision not supported in gRPC mode yet".to_string())
        }
    }

    /// Show status of local workspace
    pub fn show_workspace_status(&self) {
        println!("\n📁 Workspace Status:");
        println!("   Workspace: {:?}", self.workspace_root);
        println!("   Local files: {}", self.local_files.len());
        
        for (depot_path, state) in &self.local_files {
            let status = if state.is_modified { "MODIFIED" } else { "CLEAN" };
            println!("     - {} [rev {}] {}", depot_path, state.current_revision, status);
        }
    }

    /// Show status of server depot
    pub fn show_server_status(&self) {
        println!("\n🖥️  Server Status:");
        println!("   Depot: {:?}", self.server_depot_root);
        println!("   Files: {}", self.server_files.len());
        
        for (depot_path, meta_file) in &self.server_files {
            let latest_rev = meta_file.revisions.last()
                .map(|r| r.revision)
                .unwrap_or(0);
            let locked_status = if meta_file.locked_by.is_empty() {
                "unlocked"
            } else {
                &format!("locked by {}", meta_file.locked_by)
            };
            println!("     - {} [latest: rev {}, total: {} revisions, {}]", 
                depot_path, latest_rev, meta_file.revisions.len(), locked_status);
        }
        
        println!("   Changelists: {}", self.changelists.len());
    }

    /// Get file content for inspection
    pub fn get_local_file_content(&self, depot_path: &str) -> io::Result<String> {
        let local_state = self.local_files.get(depot_path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not checked out"))?;
        fs::read_to_string(&local_state.local_path)
    }

    /// Mark local file as modified (for testing)
    pub fn mark_file_modified(&mut self, depot_path: &str) {
        if let Some(state) = self.local_files.get_mut(depot_path) {
            state.is_modified = true;
        }
    }

    // ========== Hive 相关方法（通过 Edge 转发）==========

    /// 连接到 Hive 服务器（通过 Edge）
    /// 
    /// # Arguments
    /// * `hive_addr` - Hive 服务器地址（例如: "http://127.0.0.1:34560"）
    pub async fn connect_hive(&mut self, hive_addr: &str) -> Result<HiveConnectRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(HiveConnectReq {
            hive_address: hive_addr.to_string(),
        });
        
        let response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .hive_connect(request)
            .await?;
        
        let rsp = response.into_inner();
        
        if rsp.success {
            println!("✅ {}", rsp.message);
        } else {
            println!("❌ {}", rsp.message);
        }
        
        Ok(rsp)
    }

    /// 登录到 Hive 服务器（通过 Edge）
    /// 
    /// # Arguments
    /// * `username` - 用户名
    /// * `password` - 密码
    pub async fn hive_login(&mut self, username: String, password: String) -> Result<HiveLoginRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(HiveLoginReq {
            username: username.clone(),
            password,
        });
        
        let response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .hive_login(request)
            .await?;
        
        let login_rsp = response.into_inner();
        
        if login_rsp.success {
            println!("✅ {}", login_rsp.message);
            println!("  Access Token: {}...", &login_rsp.access_token[..20.min(login_rsp.access_token.len())]);
            println!("  Expires At: {}", login_rsp.expires_at);
        } else {
            println!("❌ {}", login_rsp.message);
        }
        
        Ok(login_rsp)
    }

    /// 注册新用户到 Hive 服务器（通过 Edge）
    /// 
    /// # Arguments
    /// * `username` - 用户名
    /// * `password` - 密码
    /// * `email` - 电子邮件
    pub async fn hive_register(&mut self, username: String, password: String, email: String) -> Result<HiveRegisterRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(HiveRegisterReq {
            username: username.clone(),
            password,
            email,
        });
        
        let response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .hive_register(request)
            .await?;
        
        let register_rsp = response.into_inner();
        
        if register_rsp.success {
            println!("✅ {}", register_rsp.message);
        } else {
            println!("❌ {}", register_rsp.message);
        }
        
        Ok(register_rsp)
    }

    /// 从 Hive 服务器获取工作空间列表（通过 Edge）
    /// 
    /// # Arguments
    /// * `name` - 可选的工作空间名称过滤
    /// * `owner` - 可选的所有者过滤
    pub async fn hive_list_workspaces(
        &mut self,
        name: Option<String>,
        owner: Option<String>,
        device_finger_print: Option<String>,
    ) -> Result<HiveListWorkspacesRsp, Box<dyn std::error::Error>> {
        let request = tonic::Request::new(HiveListWorkspacesReq {
            name,
            owner,
            device_finger_print,
        });
        
        let response = self.grpc_client.as_mut()
            .ok_or("gRPC client not initialized")?
            .hive_list_workspaces(request)
            .await?;
        
        let list_rsp = response.into_inner();
        
        if list_rsp.success {
            println!("📋 工作空间列表 ({} 个):", list_rsp.workspaces.len());
            for (idx, ws) in list_rsp.workspaces.iter().enumerate() {
                println!("  {}. {} (owner: {}, path: {})", 
                    idx + 1, ws.name, ws.owner, ws.path);
            }
        } else {
            println!("❌ {}", list_rsp.message);
        }
        
        Ok(list_rsp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_version_control_simulation() -> Result<(), Box<dyn std::error::Error>> {
        
        if std::env::var("GITHUB_ACTIONS").is_ok() || std::env::var("CI").is_ok() {
            eprintln!("Skip test_version_control_simulation on GitHub Actions CI");
            return Ok(());
        }
        
        // Setup test directories
        let workspace_root = PathBuf::from("test_workspace");
        let server_root = PathBuf::from("test_server");

        // Clean up from previous runs
        let _ = fs::remove_dir_all(&workspace_root);
        let _ = fs::remove_dir_all(&server_root);

        // Create client using new_grpc (will fail if no server, so we catch it)
        // In real usage, this would connect to a real server
        let mut client = match CrvClient::new_grpc("http://127.0.0.1:34562").await {
            Ok(mut c) => {
                // Successfully connected to server, enable local simulation
                c.enable_local_simulation(&workspace_root, &server_root)?;
                c
            }
            Err(_) => {
                // No server available, fall back to pure local simulation for testing
                println!("⚠️  No gRPC server available, using pure local simulation mode for testing");
                CrvClient::new(&workspace_root, &server_root)?
            }
        };

        // Initialize server with sample data
        client.init_server_with_sample_data()?;
        client.show_server_status();

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 1: Get latest files from server");
        println!("{}", "=".repeat(60));
        let files = client.get_latest().await?;
        println!("Available files: {:?}", files);
        assert_eq!(files.len(), 3);

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 2: Checkout file1.txt (latest version)");
        println!("{}", "=".repeat(60));
        let result = client.checkout("file1.txt").await?;
        println!("{}", result);
        
        let content = client.get_local_file_content("file1.txt")?;
        println!("Content: {}", content);
        assert!(content.contains("Version 3"));

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 3: Change to revision 1 of file1.txt");
        println!("{}", "=".repeat(60));
        let result = client.change_revision("file1.txt", 1)?;
        println!("{}", result);
        
        let content = client.get_local_file_content("file1.txt")?;
        println!("Content: {}", content);
        assert!(content.contains("Version 1"));

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 4: Modify file locally and submit");
        println!("{}", "=".repeat(60));
        let local_file = workspace_root.join("file1.txt");
        fs::write(&local_file, "Version 4 - User modified content\nNew features added")?;
        
        let result = client.submit("file1.txt", "User modification".to_string()).await?;
        println!("{}", result);

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 5: Checkout docs/readme.md");
        println!("{}", "=".repeat(60));
        let result = client.checkout("docs/readme.md").await?;
        println!("{}", result);
        
        let content = client.get_local_file_content("docs/readme.md")?;
        println!("Content: {}", content);
        assert!(content.contains("README v4"));

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 6: Check workspace status");
        println!("{}", "=".repeat(60));
        client.show_workspace_status();

        println!("\n{}", "=".repeat(60));
        println!("🧪 Test 7: Check server status");
        println!("{}", "=".repeat(60));
        client.show_server_status();

        // Verify file1.txt now has 4 revisions on server
        let meta_file = client.server_files.get("file1.txt").unwrap();
        assert_eq!(meta_file.revisions.len(), 4);
        assert_eq!(meta_file.revisions.last().unwrap().revision, 4);

        println!("\n✅ All tests passed!");

        // Cleanup
        let _ = fs::remove_dir_all(&workspace_root);
        let _ = fs::remove_dir_all(&server_root);

        Ok(())
    }
}

