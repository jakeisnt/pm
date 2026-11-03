# pm — Project Manager

A Rust CLI tool. `p` is the binary. Manages project switching, per-project knowledge, and project discovery. This repository is public — never create new branches, always work on and push directly to `main`, and do not push secrets.

## Quick Start

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
cargo install --path .
```

Rust is pinned in `rust-toolchain.toml`; do not change the toolchain casually.

## Structure

- `src/main.rs` — entry point and project commands
- `src/cli.rs` — clap definitions
- `src/project.rs` — `Project`, name → local checkout resolution, project picker
- `src/db.rs` — SQLite project index
- `src/git.rs` — the only place that shells out to git
- `src/github.rs` — GitHub REST client; `src/github_auth.rs` — token storage and login
- `src/ui.rs` — fuzzy picker, prompts, styling, status lines
- `src/shell.rs` — shell hooks installed by `p hook-install`
- `src/worktree/` — `p worktree` commands; `hooks.rs` (worktrunk config/hooks), `template.rs`
- `migrations/` — historical SQLite schema files for compatibility/reference
- `web/` — standalone static promo site for Cloudflare Pages; keep it build-free unless requested
- `config.json` — local/default configuration

## Conventions

- Use `anyhow` for command error propagation.
- Use `clap` derive for CLI parsing.
- Use `rusqlite` for SQLite access.
- Keep runtime state out of git.
- Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` after changes.
- After every update, completely rebuild and reinstall with `cargo clean && cargo build && cargo install --path .`.
