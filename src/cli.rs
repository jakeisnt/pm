use clap::{Parser, Subcommand};

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
    #[arg(short = 'a', long)]
    pub app: Option<String>,
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
