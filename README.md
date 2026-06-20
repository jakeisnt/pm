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

# Resolve a project path
p resolve pm

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

## License

[MIT](LICENSE)
