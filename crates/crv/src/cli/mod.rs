use clap::{Parser, Subcommand};

pub mod config;
pub mod output;

/// Chronoverse client — p4-compatible version control.
#[derive(Parser)]
#[command(name = "crv", version, about = "Chronoverse version control client", long_about = None)]
pub struct Cli {
    /// Server URL (e.g. http://localhost:3000)
    #[arg(short = 'p', long = "port", env = "CRV_SERVER_URL", default_value = "http://localhost:3000")]
    pub server_url: String,

    /// Auth ticket for authentication
    #[arg(short = 'P', long = "ticket", env = "CRV_TICKET")]
    pub ticket: Option<String>,

    /// Client/workspace name
    #[arg(short = 'c', long = "client", env = "CRV_CLIENT")]
    pub client_name: Option<String>,

    /// Output in tagged format (Python dict style)
    #[arg(short = 'G', long = "python")]
    pub python_output: bool,

    /// Output in tagged format
    #[arg(short = 'Z', long = "tag")]
    pub tagged_output: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Authenticate with the server
    Login {
        /// Username
        #[arg(short = 'u')]
        user: Option<String>,
    },

    /// End authenticated session
    Logout,

    /// Show server connection info
    Info,

    // ── Workspace ────────────────────────────────────
    /// Create or edit a client workspace specification
    Client {
        /// Client workspace name
        name: Option<String>,
        #[arg(short = 'r', long = "root")]
        root: Option<String>,
        #[arg(short = 's', long = "stream")]
        stream: Option<String>,
        #[arg(short = 'd')]
        delete: bool,
    },

    /// List available clients/workspaces
    Clients {
        /// Filter by user
        #[arg(short = 'u')]
        user: Option<String>,
    },

    // ── File operations ──────────────────────────────
    /// Open files for add to the depot
    Add {
        /// File pattern(s) to open for add
        files: Vec<String>,
        /// Changelist number
        #[arg(short = 'c')]
        change: Option<i64>,
        /// File type
        #[arg(short = 't')]
        file_type: Option<String>,
    },

    /// Open files for edit (checkout)
    Edit {
        /// File pattern(s) to open for edit
        files: Vec<String>,
        /// Changelist number
        #[arg(short = 'c')]
        change: Option<i64>,
    },

    /// Open files for delete from depot
    Delete {
        /// File pattern(s) to open for delete
        files: Vec<String>,
        /// Changelist number
        #[arg(short = 'c')]
        change: Option<i64>,
    },

    /// Revert opened files
    Revert {
        /// File pattern(s) to revert
        files: Vec<String>,
        /// Changelist number
        #[arg(short = 'c')]
        change: Option<i64>,
        /// Revert only unchanged files
        #[arg(short = 'a')]
        unchanged_only: bool,
    },

    /// Synchronize workspace with depot
    Sync {
        /// File pattern(s) to sync
        files: Vec<String>,
        /// Force resync even if already have revision
        #[arg(short = 'f')]
        force: bool,
        /// Preview only — don't transfer files
        #[arg(short = 'n')]
        preview: bool,
        /// Keep existing workspace files (dry-run update have list)
        #[arg(short = 'k')]
        keep_working: bool,
    },

    // ── Changelist ───────────────────────────────────
    /// Submit a changelist to the depot
    Submit {
        /// Changelist description
        #[arg(short = 'd')]
        description: Option<String>,
        /// Changelist number to submit (default: default changelist)
        #[arg(short = 'c')]
        change: Option<i64>,
    },

    /// List submitted and pending changelists
    Changes {
        /// File pattern(s)
        files: Vec<String>,
        /// Maximum number of changelists
        #[arg(short = 'm')]
        max: Option<i64>,
        /// Show changelist status
        #[arg(short = 's')]
        status: Option<String>,
        /// Client/workspace filter
        #[arg(short = 'c')]
        client: Option<String>,
        /// User filter
        #[arg(short = 'u')]
        user: Option<String>,
        /// Long output format
        #[arg(short = 'l')]
        long: bool,
    },

    /// Describe a changelist
    Describe {
        /// Changelist number
        change: i64,
        /// Show diff with previous revision
        #[arg(short = 's')]
        show_diff: bool,
    },

    // ── Inspection ───────────────────────────────────
    /// Show file diff
    Diff {
        /// File(s) to diff
        files: Vec<String>,
        /// Diff against specific revision
        #[arg(short = 'r')]
        revision: Option<String>,
    },

    /// Show revision history of files
    Filelog {
        /// File pattern(s)
        files: Vec<String>,
        /// Maximum revisions
        #[arg(short = 'm')]
        max: Option<i64>,
        /// Long output
        #[arg(short = 'l')]
        long: bool,
    },

    /// Show file status information
    Fstat {
        /// File pattern(s)
        files: Vec<String>,
    },

    /// List files opened in pending changelists
    Opened {
        /// File pattern(s)
        files: Vec<String>,
        /// Changelist filter
        #[arg(short = 'c')]
        change: Option<i64>,
    },

    /// List files synced to workspace
    Have {
        /// File pattern(s)
        files: Vec<String>,
    },

    // ── Branch & Integrate ───────────────────────────
    /// Create or edit a branch specification
    Branch {
        /// Branch name
        name: Option<String>,
        #[arg(short = 'd')]
        delete: bool,
    },

    /// List branch specifications
    Branches,

    /// Integrate (branch/merge) changes between paths
    Integrate {
        /// Source file pattern
        #[arg(short = 's')]
        source: String,
        /// Target file pattern
        #[arg(short = 't')]
        target: String,
        /// Integration action
        #[arg(short = 'a')]
        action: Option<String>,
        /// Changelist number
        #[arg(short = 'c')]
        change: Option<i64>,
    },

    /// Resolve integration conflicts
    Resolve {
        /// File pattern(s)
        files: Vec<String>,
        /// Accept source ('ay'), target ('at'), or merge ('am')
        #[arg(short = 'a')]
        accept: Option<String>,
    },

    // ── Label ────────────────────────────────────────
    /// Create or edit a label specification
    Label {
        /// Label name
        name: Option<String>,
        #[arg(short = 'd')]
        delete: bool,
    },

    /// List label specifications
    Labels,

    /// Synchronize label contents
    LabelSync {
        /// Label name
        label: String,
        /// File pattern(s)
        files: Vec<String>,
    },

    // ── Lock ─────────────────────────────────────────
    /// Lock files to prevent other users from submitting
    Lock {
        /// File pattern(s)
        files: Vec<String>,
    },

    /// Unlock previously locked files
    Unlock {
        /// File pattern(s)
        files: Vec<String>,
    },

    /// List locked files
    Locks {
        /// File pattern(s)
        files: Vec<String>,
    },

    // ── User & Group ─────────────────────────────────
    /// Create or edit a user
    User {
        /// Username
        name: Option<String>,
        #[arg(short = 'd')]
        delete: bool,
    },

    /// List users
    Users,

    /// Create or edit a group
    Group {
        /// Group name
        name: Option<String>,
        #[arg(short = 'd')]
        delete: bool,
    },

    /// List groups
    Groups,

    // ── Protection ───────────────────────────────────
    /// Create or edit a protection entry
    Protect,

    /// List protection table
    Protects,

    // ── Stream ───────────────────────────────────────
    /// Create or edit a stream specification
    Stream {
        /// Stream name
        name: Option<String>,
        #[arg(short = 'd')]
        delete: bool,
    },

    /// List stream specifications
    Streams,

    // ── Daemon ───────────────────────────────────────
    /// Start the REST daemon mode
    Daemon {
        /// Port for the local daemon server
        #[arg(short = 'p', default_value = "4000")]
        port: u16,
        /// Server URL to connect to
        #[arg(short = 's')]
        server: Option<String>,
    },
}
