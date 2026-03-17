# pm Roadmap

## 1. GUI / cmd-K Search Bar

A Raycast-style floating search for projects. Could be a standalone Electron/Tauri window or a terminal UI (Ink/blessed). The key value: instant project switching without opening a terminal first.

## 2. Local Repo Hygiene

- **Uncommitted changes**: scan all tracked projects, flag dirty working trees
- **Unpushed commits**: branches ahead of remote
- **Stale branches**: local branches merged or abandoned
- **Diverged remotes**: local main behind origin/main
- `p health` or `p doctor` — one command, dashboard of all repos

## 3. Project Activity / Recency

- Track last-opened timestamps (we already have a DB)
- `p recent` — MRU list, sorted by last access
- Weight fuzzy search by recency so active projects surface first

## 4. Project Tagging / Grouping

- Beyond just scope (personal/work), let users tag projects arbitrarily: `p tag <project> frontend`, `p list --tag frontend`
- Useful once you have 50+ repos

## 5. Project Quickstart / Onboarding

- `p init` — detect project type, set dev command, register scan dirs
- `p open` — open in editor (Zed/Cursor) + start dev server + open browser, one command

## 6. GitHub Inbox

- `p prs` — show open PRs across all your repos (review requested, authored, Dependabot)
- `p issues` — same for issues
- A personal dashboard without leaving the terminal

## 7. Cross-Repo Operations

- `p each <cmd>` — run a command in every tracked project (git pull, bun update, etc.)
- `p each --dirty` — only repos with uncommitted changes

## 8. Project Notes / Scratchpad

- `p note <text>` — quick per-project notes stored in the DB
- Lightweight alternative to full repo memories — just "what was I doing last time I touched this"

## 9. Auto-Discovery Improvements

- Watch filesystem for new cloned repos, auto-register them
- `p sync` — reconcile DB with disk (remove dead paths, discover new ones)

## 10. Session Management

- `p session save/restore` — snapshot which projects + terminals + editors are open
- Useful for context-switching between "work mode" and "personal mode"

## 11. CLI Hooks

### Failure Hook: Claude Debugger

When any `p` command fails, optionally launch an interactive Claude Code debugger session immediately with full debug context — the failing command, stderr/stdout, stack trace, relevant log output, and environment info. This should be wired in at the top-level error boundary so every command gets it for free. The hook is opt-in via `p config set hooks.on-fail claude-debug` (or similar).

### Unknown Command Hook: Wish Listener

When a user runs a command that doesn't exist (e.g. `p poop`), instead of just printing "unknown command", interactively prompt them: "That command doesn't exist. What do you wish it did?" Their answer gets tracked as a feature wish on a project board (stored in the DB). This turns typos and half-formed ideas into a backlog. The hook is opt-in via `p config set hooks.on-unknown wish`.

## Priority

1. **Hooks** (#11) — the failure debugger pays for itself immediately; the wish listener builds the backlog organically
2. **Repo hygiene** (#2) and **recency** (#3) — highest daily leverage
3. **GUI** (#1) — flashy but big lift; consider whether Raycast integration is cheaper than building our own
4. **GitHub inbox** (#6) and **cross-repo ops** (#7) — round out the "command center" vision
5. Everything else as needed
