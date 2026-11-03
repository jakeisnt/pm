# pm

A fast Rust project manager CLI for switching between repositories, tracking recent projects, and organizing work across GitHub orgs.

## Install

```bash
cargo install --path .
```

The CLI binary is `p`.

## Nix

The repository provides a flake package. To use `p` in another flake, add it as an input and include the package in your development shell:

```nix
{
  inputs.pm.url = "github:jakeisnt/pm";

  outputs = { nixpkgs, pm, ... }:
    let
      system = "aarch64-darwin"; # or x86_64-linux, etc.
      pkgs = import nixpkgs { inherit system; };
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [ pm.packages.${system}.default ];
      };
    };
}
```

For a one-off install, use `nix profile install github:jakeisnt/pm`. The package can also be run directly with `nix run github:jakeisnt/pm`.

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

# Rename a project locally and on GitHub
# First argument: current name. Second argument: desired new name.
p rename <old-name> <new-name>
p r <old-name> <new-name>

# Worktrees (worktrunk-style; `p wt` is short for `p worktree`)
p wt                          # list worktrees with status
p wt switch feat              # switch to feat's worktree, creating it if the branch exists
p wt sw -c feat               # create branch + worktree from the default branch
p wt sw -c feat -x claude -- 'Fix the pagination bug'   # create, then start an agent there
p wt sw                       # fuzzy-pick a worktree or branch in this project
p wt -P pm sw                 # ...in another project (same lookup as `p pm`)
p wt sw ^                     # default branch; `-` = previous worktree
p wt merge                    # squash, rebase, fast-forward into main, remove worktree
p wt rm                       # remove the current worktree (and its branch, if merged)

# Move a linked worktree branch back onto the primary checkout
p worktree checkout <branch-name>
p worktree co <branch-name>

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

### Worktrees

`p worktree` (alias `p wt`) is modelled on [worktrunk](https://worktrunk.dev): worktrees are addressed by branch name and their paths come from a template.

Worktree commands act on the project containing the current directory. From anywhere else, pass `-P/--project <name>`, which is resolved like `p <name>`: a name that matches nothing opens the project picker.

- `switch` (`sw`, `s`) goes to the branch's worktree, creating one when the branch exists locally or on `origin`. `--create` makes a new branch from `--base` (default: the repo's default branch). `-x <cmd>` runs a command in the worktree (arguments after `--` are passed to it). `-p` prints the path instead. With no branch, or one that matches nothing, it opens a fuzzy picker of the project's worktrees and branches.
- `list` (`ls`) shows `@` current / `^` primary, status (`+` staged, `!` modified, `?` untracked), uncommitted line counts, `↑`/`↓` vs the default branch, `⇡`/`⇣` vs upstream, and the last commit. `--branches` includes branches without a worktree; `--json` for scripting.
- `merge [target]` requires a clean worktree, rebases onto the target, squashes multiple commits (`--no-squash` to keep them, `-m` for the message), fast-forwards the target, then removes the worktree and branch (`--no-remove` to keep them).
- `remove` (`rm`) removes the current (or named) worktree and deletes its branch when it is already merged into the default branch. `-f` removes dirty worktrees; `-D` deletes unmerged branches.

**worktrunk compatibility.** `p` reads worktrunk's own config, so repos already set up for `wt` work unchanged:

- `worktree-path` in `~/.config/worktrunk/config.toml` (default `{{ repo_path }}/../{{ repo }}.{{ branch | sanitize }}`, giving `~/code/repo.feature-x`).
- Hooks from `.config/wt.toml` (project) and the user config, in any of worktrunk's string / table / `[[pipeline]]` forms: `pre-start` (also `post-create`), `post-start`, `post-switch`, `pre-merge`, `post-merge`, `pre-remove`, `post-remove`. `pre-*` hooks block and abort on failure; `post-*` hooks run in the background and log to `.git/pm-logs/`. Project hooks need approval the first time (`--yes` to approve without prompting); approvals are saved in the repo's local git config.
- Templates support worktrunk's common variables (`branch`, `worktree_path`, `worktree_name`, `repo`, `repo_path`, `default_branch`, `base`, `target`, `commit`, ...) and the filters `sanitize`, `basename`, `dirname`, `hash_port`, `lower` and `upper`. `{% ... %}` blocks are not supported. `hash_port` yields a stable port in 10000–19999, but not the same number worktrunk would pick.

To change the shell's directory, `switch`, `merge` and `remove` rely on the shell integration installed by `p hook-install`. Without it, `switch` opens a subshell in the worktree, the same way `p <project>` does.

### Shell hook

`p hook-install` installs an idempotent hook block in your shell startup file (`.zshrc`, `.bashrc`, or `~/.config/fish/conf.d/p.fish`). The hook wraps `git` and only runs after successful repository-creating commands such as `git init`, `git clone`, and `git worktree add`; it does not run on prompt or directory changes. When it runs, it calls `p index --quiet <repo-path>` in the background. The hook also defines a small `p` wrapper function so `p worktree` commands can `cd` your shell into the destination worktree. `p index <repo-path>` records that local repository and, when `origin` points at GitHub, stores the `owner/repo` remote in the index too. Running `p index` without a path forces a full reindex of configured local roots plus GitHub repositories accessible to your authenticated account.

## License

[MIT](LICENSE)
