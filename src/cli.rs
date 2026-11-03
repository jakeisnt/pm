use clap::{Args, Parser, Subcommand, ValueEnum};

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
        /// Refused: public repositories must be created manually outside p.
        #[arg(long, conflicts_with = "private")]
        public: bool,
        /// Create a private repository. This is always the default.
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
    /// Rename a project locally and on GitHub.
    #[command(alias = "r")]
    Rename {
        /// Current project name.
        old_name: String,
        /// Desired new project name.
        new_name: String,
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
    /// Force reindexing of local roots and GitHub remotes, or index one git repository path.
    Index {
        /// Optional repository path to index; when omitted, all configured local roots and GitHub remotes are indexed.
        path: Option<String>,
        #[arg(short, long)]
        quiet: bool,
    },
    /// Manage git worktrees (worktrunk-style); `p wt` with no subcommand lists them.
    #[command(alias = "wt")]
    Worktree {
        /// Project whose worktrees to manage (same lookup as `p <name>`).
        /// Defaults to the project containing the current directory.
        #[arg(short = 'P', long, global = true)]
        project: Option<String>,
        #[command(subcommand)]
        command: Option<WorktreeCmd>,
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

#[derive(Subcommand)]
pub enum WorktreeCmd {
    /// Switch to a branch's worktree, creating the worktree if needed.
    ///
    /// Accepts a branch name, a worktree path, `^` (default branch), `@`
    /// (current) or `-` (previous). With no branch, or one that doesn't
    /// exist, fuzzy-finds the project's worktrees and branches.
    /// With the shell integration from `p hook-install` the shell changes
    /// directory; otherwise a subshell opens in the worktree.
    #[command(visible_aliases = ["sw", "s"])]
    Switch(SwitchArgs),
    /// List worktrees with status, ahead/behind counts and last commit.
    #[command(visible_alias = "ls")]
    List(ListArgs),
    /// Remove worktrees (default: the current one) and their merged branches.
    #[command(visible_alias = "rm")]
    Remove(RemoveArgs),
    /// Squash, rebase and fast-forward the current branch into the target
    /// (default: the default branch), then remove the worktree.
    Merge(MergeArgs),
    /// Move a linked worktree's branch (and its changes) onto the primary checkout.
    #[command(alias = "co")]
    Checkout { branch_name: String },
}

/// Hook options shared by every worktree command that runs hooks.
#[derive(Args, Clone, Copy, Default)]
pub struct HookArgs {
    /// Approve project hooks from .config/wt.toml without prompting.
    #[arg(short, long)]
    pub yes: bool,
    /// Skip hooks.
    #[arg(long)]
    pub no_hooks: bool,
}

#[derive(Args)]
pub struct SwitchArgs {
    pub branch: Option<String>,
    /// Create a new branch (from --base, default: the default branch).
    #[arg(short, long)]
    pub create: bool,
    /// Base branch for --create.
    #[arg(short, long)]
    pub base: Option<String>,
    /// Command to run in the worktree after switching, e.g. `-x claude`.
    #[arg(short = 'x', long)]
    pub execute: Option<String>,
    /// Print the worktree path instead of switching to it.
    #[arg(short, long)]
    pub print_path: bool,
    #[command(flatten)]
    pub hooks: HookArgs,
    /// Arguments passed to the --execute command.
    #[arg(last = true)]
    pub args: Vec<String>,
}

#[derive(Args, Default)]
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
    /// Also show local branches that have no worktree.
    #[arg(long)]
    pub branches: bool,
}

#[derive(Args)]
pub struct RemoveArgs {
    pub branches: Vec<String>,
    /// Remove even when the worktree has uncommitted changes.
    #[arg(short, long)]
    pub force: bool,
    /// Delete the branch even when it isn't merged.
    #[arg(short = 'D', long)]
    pub force_delete: bool,
    /// Keep the branch.
    #[arg(long)]
    pub no_delete_branch: bool,
    #[command(flatten)]
    pub hooks: HookArgs,
}

#[derive(Args)]
pub struct MergeArgs {
    pub target: Option<String>,
    /// Keep individual commits instead of squashing.
    #[arg(long)]
    pub no_squash: bool,
    /// Keep the worktree and branch after merging.
    #[arg(long)]
    pub no_remove: bool,
    /// Commit message for the squashed commit.
    #[arg(short, long)]
    pub message: Option<String>,
    #[command(flatten)]
    pub hooks: HookArgs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_accepts_long_name_and_short_alias() {
        for command in ["rename", "r"] {
            let cli =
                Cli::try_parse_from(["p", command, "old", "new"]).expect("parse rename command");
            assert!(matches!(
                cli.command,
                Some(Commands::Rename { old_name, new_name })
                    if old_name == "old" && new_name == "new"
            ));
        }
    }

    #[test]
    fn worktree_switch_parses_create_execute_and_trailing_args() {
        let cli = Cli::try_parse_from(["p", "wt", "sw", "-c", "-x", "claude", "feat", "--", "go"])
            .expect("parse switch");
        assert!(matches!(
            cli.command,
            Some(Commands::Worktree {
                project: None,
                command: Some(WorktreeCmd::Switch(SwitchArgs { branch: Some(b), create: true, execute: Some(x), args, .. }))
            }) if b == "feat" && x == "claude" && args == ["go"]
        ));
        let cli = Cli::try_parse_from(["p", "worktree"]).expect("parse bare worktree");
        assert!(matches!(
            cli.command,
            Some(Commands::Worktree {
                project: None,
                command: None
            })
        ));
        for argv in [
            &["p", "wt", "-P", "pm", "sw"][..],
            &["p", "wt", "sw", "--project", "pm"][..],
        ] {
            let cli = Cli::try_parse_from(argv).expect("parse --project");
            assert!(matches!(
                cli.command,
                Some(Commands::Worktree { project: Some(p), command: Some(WorktreeCmd::Switch(SwitchArgs { branch: None, .. })) })
                    if p == "pm"
            ));
        }
    }
}
