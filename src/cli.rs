use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "p",
    version,
    about = "Project manager — switch between projects quickly"
)]
pub struct Cli {
    pub name: Option<String>,
    #[arg(short = 'p', long)]
    pub path: bool,
    #[arg(short, long)]
    pub open: Option<String>,
    #[arg(long)]
    pub app: Option<String>,
    #[arg(short = 'a', long)]
    pub agent: bool,
    #[arg(short, long)]
    pub silent: bool,
    #[arg(long)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    Resolve {
        name: String,
    },
    /// Create a new GitHub repository and clone it into the standard local path.
    Create {
        /// Repository name, or owner/name for an org/user repository.
        name: String,
        /// Create a public repository. Repositories are private by default.
        #[arg(long, conflicts_with = "private")]
        public: bool,
        /// Create a private repository. This is the default.
        #[arg(long)]
        private: bool,
        /// Optional GitHub repository description.
        #[arg(short, long)]
        description: Option<String>,
    },
    List {
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Remove {
        path: Option<String>,
        #[arg(short, long)]
        force: bool,
    },
    Dev,
    Config {
        #[command(subcommand)]
        command: ConfigCmd,
    },
    Org {
        #[command(subcommand)]
        command: OrgCmd,
    },
    Github {
        #[command(subcommand)]
        command: GithubCmd,
    },
    /// Print a shell hook that indexes git repositories after git creates them.
    Hook {
        #[arg(value_enum, default_value_t = Shell::Zsh)]
        shell: Shell,
    },
    /// Install or update the shell hook in your shell startup file.
    HookInstall {
        #[arg(value_enum)]
        shell: Option<Shell>,
    },
    /// Index a git repository path. Intended for shell hooks.
    Index {
        path: Option<String>,
        #[arg(short, long)]
        quiet: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    List,
    Set { key: String, value: String },
    Delete { key: String },
}

#[derive(Subcommand)]
pub enum OrgCmd {
    List,
    Hide { name: String },
    Show { name: String },
}

#[derive(Subcommand)]
pub enum GithubCmd {
    /// Authenticate with GitHub using the OAuth device flow.
    Login {
        /// GitHub OAuth app client id. Falls back to PM_GITHUB_CLIENT_ID.
        #[arg(long)]
        client_id: Option<String>,
    },
    /// Show the saved GitHub authentication status.
    Status,
    /// Remove saved GitHub credentials.
    Logout,
}
