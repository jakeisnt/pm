# pm

A fast Rust project manager CLI for switching between repositories, tracking recent projects, and organizing work across GitHub orgs.

## Install

```bash
cargo install --path .
```

The CLI binary is `p`.

## Development

This repository pins Rust in `rust-toolchain.toml`.

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Usage

```bash
# Interactive project switcher
p

# Jump directly to a project; if no match is found, opens the selector with this query prefilled
p pm

# Open pi in the selected project
p --agent pm
p -a

# Resolve a project path
p resolve pm

# Create a new private GitHub repo and clone it under ~/Documents/<owner>/<repo>
p create my-new-repo
p create ulnd/my-new-repo --description "Demo project"
# Public repositories are intentionally refused by p; create those manually.

# List all projects
p list
p list --json
p list --source local

# Remove a project from disk
p remove /path/to/project --force

# Manage org visibility
p org list
p org hide <org>
p org show <org>

# Authenticate GitHub access for remote repository lookup/clone
PM_GITHUB_CLIENT_ID=<oauth-client-id> p github login
p github status
p github logout

# Install a shell hook that indexes repositories after git init/clone/worktree add
p hook-install
# Or choose a shell explicitly
p hook-install zsh
p hook-install bash
p hook-install fish

# Print the hook without installing it
p hook zsh

# Force a full reindex of configured local roots and accessible GitHub remotes
p index

# Index one git repository manually (used by shell hooks)
p index /path/to/repo

# Manage config.json
p config list
p config set searchDepth 3
p config delete searchDepth
```

## Configuration

`config.json` in the current checkout supports:

- `searchRoots`: array of directories to scan for Git repositories
- `searchDepth`: recursion depth for project discovery

Runtime state is stored in SQLite under the platform data directory (`pm/pm.db`). GitHub OAuth tokens are stored separately in the platform config directory (`pm/github-token`) with user-only permissions on Unix.

`p hook-install` installs an idempotent hook block in your shell startup file (`.zshrc`, `.bashrc`, or `~/.config/fish/conf.d/p.fish`). The hook wraps `git` and only runs after successful repository-creating commands such as `git init`, `git clone`, and `git worktree add`; it does not run on prompt or directory changes. When it runs, it calls `p index --quiet <repo-path>` in the background. `p index <repo-path>` records that local repository and, when `origin` points at GitHub, stores the `owner/repo` remote in the index too. Running `p index` without a path forces a full reindex of configured local roots plus GitHub repositories accessible to your authenticated account.

## License

[MIT](LICENSE)
