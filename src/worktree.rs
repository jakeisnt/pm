//! Worktree management modelled on worktrunk (https://worktrunk.dev).
//!
//! Worktrees are addressed by branch name and their paths come from a template.
//! `p worktree` reads worktrunk's own config files, so repos already set up for
//! `wt` work unchanged:
//!
//! - `~/.config/worktrunk/config.toml`: user config (`worktree-path`, user hooks)
//! - `.config/wt.toml`: project hooks (`pre-start`, `post-start`, `pre-merge`, ...)

use crate::cli::WorktreeCmd;
use anyhow::{Context, Result, bail};
use colored::Colorize;
use regex::Regex;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

/// When set (by the shell integration from `p hook-install`), commands that
/// change worktrees write the destination path here and the shell `cd`s to it.
const CD_FILE_ENV: &str = "PM_CD_FILE";
const DEFAULT_WORKTREE_PATH: &str = "{{ repo_path }}/../{{ repo }}.{{ branch | sanitize }}";
const PREVIOUS_BRANCH_KEY: &str = "pm.previousBranch";
const APPROVED_HOOK_KEY: &str = "pm.approvedHook";

pub fn run(cmd: Option<WorktreeCmd>) -> Result<()> {
    match cmd.unwrap_or(WorktreeCmd::List {
        json: false,
        branches: false,
    }) {
        WorktreeCmd::Checkout { branch_name } => checkout(&branch_name),
        WorktreeCmd::List { json, branches } => list(json, branches),
        WorktreeCmd::Switch {
            branch,
            create,
            base,
            execute,
            print_path,
            yes,
            no_hooks,
            args,
        } => switch(SwitchOpts {
            branch,
            create,
            base,
            execute,
            print_path,
            yes,
            no_hooks,
            args,
        }),
        WorktreeCmd::Remove {
            branches,
            force,
            force_delete,
            no_delete_branch,
            yes,
            no_hooks,
        } => remove(RemoveOpts {
            branches,
            force,
            force_delete,
            no_delete_branch,
            yes,
            no_hooks,
        }),
        WorktreeCmd::Merge {
            target,
            no_squash,
            no_remove,
            message,
            yes,
            no_hooks,
        } => merge(MergeOpts {
            target,
            no_squash,
            no_remove,
            message,
            yes,
            no_hooks,
        }),
    }
}

// ---------------------------------------------------------------------------
// Repository model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

struct Repo {
    worktrees: Vec<Worktree>,
    primary: Worktree,
    current: Option<Worktree>,
    common_dir: PathBuf,
    default_branch: String,
}

impl Repo {
    fn discover() -> Result<Self> {
        let cwd = env::current_dir()?;
        let worktrees = parse_worktrees(&git(&cwd, &["worktree", "list", "--porcelain"])?);
        if worktrees.is_empty() {
            bail!("no git worktrees found");
        }
        let common_dir = PathBuf::from(git(
            &cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?);
        let primary = worktrees
            .iter()
            .find(|w| Some(w.path.as_path()) == common_dir.parent())
            .or_else(|| worktrees.first())
            .cloned()
            .context("could not determine primary worktree")?;
        let top = git(&cwd, &["rev-parse", "--show-toplevel"])
            .ok()
            .map(PathBuf::from);
        let current =
            top.and_then(|top| worktrees.iter().find(|w| same_path(&w.path, &top)).cloned());
        let default_branch = detect_default_branch(&primary.path, &primary);
        Ok(Self {
            worktrees,
            primary,
            current,
            common_dir,
            default_branch,
        })
    }

    fn name(&self) -> String {
        file_name(&self.primary.path)
    }

    fn for_branch(&self, branch: &str) -> Option<&Worktree> {
        self.worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(branch))
    }

    fn current_or_primary(&self) -> &Worktree {
        self.current.as_ref().unwrap_or(&self.primary)
    }

    fn is_primary(&self, wt: &Worktree) -> bool {
        same_path(&wt.path, &self.primary.path)
    }

    fn is_current(&self, wt: &Worktree) -> bool {
        self.current
            .as_ref()
            .is_some_and(|c| same_path(&c.path, &wt.path))
    }

    /// Resolve a user-supplied name: `^` (default branch), `@` (current), `-`
    /// (previous), a branch name, or the path of a worktree.
    fn resolve_name(&self, name: &str) -> Result<String> {
        match name {
            "^" => Ok(self.default_branch.clone()),
            "@" => self
                .current
                .as_ref()
                .and_then(|w| w.branch.clone())
                .context("current worktree has no branch"),
            "-" => git(
                &self.primary.path,
                &["config", "--get", PREVIOUS_BRANCH_KEY],
            )
            .ok()
            .filter(|s| !s.is_empty())
            .context("no previous worktree to switch back to"),
            _ => {
                let as_path = Path::new(name);
                if as_path.exists()
                    && let Some(wt) = self.worktrees.iter().find(|w| same_path(&w.path, as_path))
                {
                    return wt
                        .branch
                        .clone()
                        .with_context(|| format!("worktree {name} is detached"));
                }
                Ok(name.to_string())
            }
        }
    }

    fn remember_previous(&self) {
        if let Some(branch) = self.current.as_ref().and_then(|w| w.branch.as_deref()) {
            let _ = git(
                &self.primary.path,
                &["config", "--local", PREVIOUS_BRANCH_KEY, branch],
            );
        }
    }

    fn base_vars(&self) -> Vars {
        let mut v = Vars::new();
        v.insert("repo".into(), self.name());
        v.insert("repo_path".into(), display(&self.primary.path));
        v.insert("primary_worktree_path".into(), display(&self.primary.path));
        v.insert("default_branch".into(), self.default_branch.clone());
        if let Ok(url) = git(&self.primary.path, &["remote", "get-url", "origin"]) {
            v.insert("remote".into(), "origin".into());
            if let Some((owner, name)) = parse_remote(&url) {
                v.insert("owner".into(), owner);
                v.insert("remote_repo".into(), name);
            }
            v.insert("remote_url".into(), url);
        }
        v
    }

    /// Variables describing `wt` as the active worktree for a hook or template.
    fn worktree_vars(&self, wt: &Worktree) -> Vars {
        let mut v = self.base_vars();
        v.insert("worktree_path".into(), display(&wt.path));
        v.insert("worktree_name".into(), file_name(&wt.path));
        if let Some(branch) = &wt.branch {
            v.insert("branch".into(), branch.clone());
            if let Ok(up) = git(
                &wt.path,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    &format!("{branch}@{{upstream}}"),
                ],
            ) {
                v.insert("upstream".into(), up);
            }
        }
        if let Ok(sha) = git(&wt.path, &["rev-parse", "HEAD"]) {
            v.insert("commit".into(), sha);
        }
        if let Ok(sha) = git(&wt.path, &["rev-parse", "--short", "HEAD"]) {
            v.insert("short_commit".into(), sha);
        }
        v
    }

    fn path_for(&self, branch: &str) -> Result<PathBuf> {
        let template = UserConfig::load()
            .worktree_path
            .unwrap_or_else(|| DEFAULT_WORKTREE_PATH.to_string());
        let mut vars = self.base_vars();
        vars.insert("branch".into(), branch.to_string());
        let rendered = render(&template, &vars).context("invalid worktree-path template")?;
        let expanded = expand_home(&rendered);
        let path = if expanded.is_absolute() {
            expanded
        } else {
            self.primary.path.join(expanded)
        };
        Ok(normalize(&path))
    }

    fn log_dir(&self) -> PathBuf {
        self.common_dir.join("pm-logs")
    }
}

pub fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut path = None;
    let mut branch = None;
    for line in porcelain.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(path) = path.take() {
                worktrees.push(Worktree {
                    path,
                    branch: branch.take(),
                });
            }
        } else if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(value));
        } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
            branch = Some(value.to_string());
        }
    }
    worktrees
}

fn detect_default_branch(path: &Path, primary: &Worktree) -> String {
    if let Ok(head) = git(
        path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && let Some(branch) = head.strip_prefix("origin/")
        && branch_exists(path, branch)
    {
        return branch.to_string();
    }
    if let Ok(configured) = git(path, &["config", "--get", "init.defaultBranch"])
        && branch_exists(path, &configured)
    {
        return configured;
    }
    for candidate in ["main", "master", "trunk", "develop"] {
        if branch_exists(path, candidate) {
            return candidate.to_string();
        }
    }
    primary.branch.clone().unwrap_or_else(|| "main".to_string())
}

fn parse_remote(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
    let path = if let Some((_, rest)) = trimmed.split_once("://") {
        rest.split_once('/')?.1
    } else {
        trimmed.split_once(':')?.1
    };
    let (owner, name) = path.rsplit_once('/')?;
    Some((owner.to_string(), name.to_string()))
}

// ---------------------------------------------------------------------------
// list
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ListRow {
    branch: Option<String>,
    path: Option<String>,
    current: bool,
    primary: bool,
    staged: bool,
    modified: bool,
    untracked: bool,
    lines_added: u64,
    lines_removed: u64,
    ahead: u64,
    behind: u64,
    upstream_ahead: Option<u64>,
    upstream_behind: Option<u64>,
    commit: String,
    commit_time: i64,
    message: String,
}

fn list(json: bool, include_branches: bool) -> Result<()> {
    let repo = Repo::discover()?;
    let mut rows: Vec<ListRow> = repo
        .worktrees
        .iter()
        .map(|wt| row_for(&repo, Some(wt), wt.branch.as_deref()))
        .collect();
    if include_branches {
        let branches = git(
            &repo.primary.path,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )?;
        for branch in branches.lines().filter(|b| repo.for_branch(b).is_none()) {
            rows.push(row_for(&repo, None, Some(branch)));
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    print_table(&repo, &rows);
    Ok(())
}

fn row_for(repo: &Repo, wt: Option<&Worktree>, branch: Option<&str>) -> ListRow {
    let dir = wt.map(|w| w.path.as_path()).unwrap_or(&repo.primary.path);
    let rev = match (wt, branch) {
        (Some(_), _) => "HEAD".to_string(),
        (None, Some(b)) => format!("refs/heads/{b}"),
        (None, None) => "HEAD".to_string(),
    };
    let (mut staged, mut modified, mut untracked) = (false, false, false);
    let (mut lines_added, mut lines_removed) = (0, 0);
    if let Some(wt) = wt {
        if let Ok(status) = git(&wt.path, &["status", "--porcelain"]) {
            (staged, modified, untracked) = parse_status(&status);
        }
        if let Ok(numstat) = git(&wt.path, &["diff", "HEAD", "--numstat"]) {
            (lines_added, lines_removed) = sum_numstat(&numstat);
        }
    }
    let (behind, ahead) = left_right(dir, &repo.default_branch, &rev).unwrap_or((0, 0));
    let (upstream_behind, upstream_ahead) = match branch {
        Some(b) => left_right(dir, &format!("{b}@{{upstream}}"), &rev)
            .map(|(l, r)| (Some(l), Some(r)))
            .unwrap_or((None, None)),
        None => (None, None),
    };
    let log = git(dir, &["log", "-1", "--format=%h%x09%ct%x09%s", &rev]).unwrap_or_default();
    let mut parts = log.splitn(3, '\t');
    let commit = parts.next().unwrap_or_default().to_string();
    let commit_time = parts.next().and_then(|t| t.parse().ok()).unwrap_or(0);
    let message = parts.next().unwrap_or_default().to_string();
    ListRow {
        branch: branch.map(str::to_string),
        path: wt.map(|w| display(&w.path)),
        current: wt.is_some_and(|w| repo.is_current(w)),
        primary: wt.is_some_and(|w| repo.is_primary(w)),
        staged,
        modified,
        untracked,
        lines_added,
        lines_removed,
        ahead,
        behind,
        upstream_ahead,
        upstream_behind,
        commit,
        commit_time,
        message,
    }
}

pub fn parse_status(porcelain: &str) -> (bool, bool, bool) {
    let (mut staged, mut modified, mut untracked) = (false, false, false);
    for line in porcelain.lines() {
        let mut chars = line.chars();
        let (x, y) = (chars.next().unwrap_or(' '), chars.next().unwrap_or(' '));
        if x == '?' {
            untracked = true;
            continue;
        }
        staged |= x != ' ';
        modified |= y != ' ';
    }
    (staged, modified, untracked)
}

fn sum_numstat(numstat: &str) -> (u64, u64) {
    numstat.lines().fold((0, 0), |(a, r), line| {
        let mut parts = line.split('\t');
        let add = parts
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0);
        let del = parts
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0);
        (a + add, r + del)
    })
}

/// Returns (commits only in `left`, commits only in `right`).
fn left_right(dir: &Path, left: &str, right: &str) -> Option<(u64, u64)> {
    let out = git(
        dir,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{left}...{right}"),
        ],
    )
    .ok()?;
    let mut parts = out.split_whitespace();
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn print_table(repo: &Repo, rows: &[ListRow]) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let default = &repo.default_branch;
    let headers = [
        "",
        "Branch",
        "Status",
        "HEAD±",
        &format!("{default}↕"),
        "Remote⇅",
        "Commit",
        "Age",
        "Message",
        "Path",
    ];
    let mut table: Vec<Vec<(String, Option<colored::Color>)>> = Vec::new();
    use colored::Color::*;
    for row in rows {
        let marker = if row.current {
            "@"
        } else if row.primary {
            "^"
        } else if row.path.is_none() {
            "/"
        } else {
            " "
        };
        let mut status = String::new();
        if row.staged {
            status.push('+');
        }
        if row.modified {
            status.push('!');
        }
        if row.untracked {
            status.push('?');
        }
        let head = match (row.lines_added, row.lines_removed) {
            (0, 0) => String::new(),
            (a, r) => format!("+{a} -{r}"),
        };
        let vs_default = if row.branch.as_deref() == Some(default.as_str()) {
            String::new()
        } else {
            join_nonempty(&[arrow('↑', row.ahead), arrow('↓', row.behind)])
        };
        let remote = join_nonempty(&[
            arrow('⇡', row.upstream_ahead.unwrap_or(0)),
            arrow('⇣', row.upstream_behind.unwrap_or(0)),
        ]);
        let path = row
            .path
            .as_deref()
            .map(|p| tildify(Path::new(p)))
            .unwrap_or_default();
        table.push(vec![
            (marker.to_string(), Some(BrightMagenta)),
            (
                row.branch.clone().unwrap_or_else(|| "(detached)".into()),
                Some(if row.current { BrightCyan } else { White }),
            ),
            (status, Some(Yellow)),
            (head, Some(Green)),
            (vs_default, Some(Cyan)),
            (remote, Some(Blue)),
            (row.commit.clone(), Some(BrightBlack)),
            (age(now - row.commit_time), Some(BrightBlack)),
            (truncate(&row.message, 50), None),
            (path, Some(BrightBlack)),
        ]);
    }

    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in &table {
        for (i, (cell, _)) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    let header_line = headers
        .iter()
        .enumerate()
        .map(|(i, h)| pad(h, widths[i]))
        .collect::<Vec<_>>()
        .join("  ");
    println!("{}", header_line.trim_end().bold());
    for row in &table {
        let line = row
            .iter()
            .enumerate()
            .map(|(i, (cell, color))| {
                let padded = pad(cell, widths[i]);
                match color {
                    Some(c) => padded.color(*c).to_string(),
                    None => padded,
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        println!("{}", line.trim_end());
    }

    let worktrees = rows.iter().filter(|r| r.path.is_some()).count();
    let changed = rows
        .iter()
        .filter(|r| r.staged || r.modified || r.untracked)
        .count();
    let ahead = rows.iter().filter(|r| r.ahead > 0).count();
    let branches = rows.len() - worktrees;
    let mut summary = format!(
        "Showing {worktrees} worktree{}",
        if worktrees == 1 { "" } else { "s" }
    );
    if branches > 0 {
        summary.push_str(&format!(", {branches} branch(es) without worktree"));
    }
    summary.push_str(&format!(", {changed} with changes, {ahead} ahead"));
    println!();
    info(&summary);
}

fn arrow(symbol: char, n: u64) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("{symbol}{n}")
    }
}

fn join_nonempty(parts: &[String]) -> String {
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    format!("{s}{}", " ".repeat(width.saturating_sub(len)))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

pub fn age(secs: i64) -> String {
    let secs = secs.max(0);
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s if s < 7 * 86_400 => format!("{}d", s / 86_400),
        s if s < 30 * 86_400 => format!("{}w", s / (7 * 86_400)),
        s if s < 365 * 86_400 => format!("{}mo", s / (30 * 86_400)),
        s => format!("{}y", s / (365 * 86_400)),
    }
}

// ---------------------------------------------------------------------------
// switch
// ---------------------------------------------------------------------------

struct SwitchOpts {
    branch: Option<String>,
    create: bool,
    base: Option<String>,
    execute: Option<String>,
    print_path: bool,
    yes: bool,
    no_hooks: bool,
    args: Vec<String>,
}

fn switch(opts: SwitchOpts) -> Result<()> {
    let repo = Repo::discover()?;
    let branch = match &opts.branch {
        Some(name) => {
            let branch = repo.resolve_name(name)?;
            let known = repo.for_branch(&branch).is_some()
                || branch_exists(&repo.primary.path, &branch)
                || remote_branch_exists(&repo.primary.path, &branch);
            if known || opts.create || !io::stdin().is_terminal() {
                branch
            } else {
                // Same as `p <name>`: no exact match opens the picker prefilled.
                warn(&format!("no worktree or branch for {}", branch.bold()));
                match pick(&repo, Some(name))? {
                    Some(branch) => branch,
                    None => return Ok(()),
                }
            }
        }
        None => match pick(&repo, None)? {
            Some(branch) => branch,
            None => return Ok(()),
        },
    };
    let hooks = !opts.no_hooks;

    let (wt, created, base) = if let Some(existing) = repo.for_branch(&branch) {
        if opts.create {
            bail!(
                "branch {branch} already has a worktree at {}",
                tildify(&existing.path)
            );
        }
        (existing.clone(), false, None)
    } else {
        let path = repo.path_for(&branch)?;
        if path.exists() {
            bail!(
                "cannot create worktree for {branch}: {} already exists",
                tildify(&path)
            );
        }
        let path_str = display(&path);
        let base_branch;
        if opts.create {
            if branch_exists(&repo.primary.path, &branch) {
                bail!("branch {branch} already exists; switch to it without --create");
            }
            base_branch = match &opts.base {
                Some(b) => repo.resolve_name(b)?,
                None => repo.default_branch.clone(),
            };
            git_quiet(
                &repo.primary.path,
                &["worktree", "add", "-b", &branch, &path_str, &base_branch],
            )?;
            success(&format!(
                "Created branch {} from {} and worktree @ {}",
                branch.bold(),
                base_branch.bold(),
                tildify(&path)
            ));
        } else if branch_exists(&repo.primary.path, &branch)
            || remote_branch_exists(&repo.primary.path, &branch)
        {
            if opts.base.is_some() {
                bail!("--base only applies with --create");
            }
            base_branch = repo.current_or_primary().branch.clone().unwrap_or_default();
            // git DWIMs `origin/<branch>` into a tracking branch when no local one exists.
            git_quiet(&repo.primary.path, &["worktree", "add", &path_str, &branch])?;
            success(&format!(
                "Created worktree for {} @ {}",
                branch.bold(),
                tildify(&path)
            ));
        } else {
            bail!(
                "no branch named {branch}; create it with {}",
                format!("p worktree switch --create {branch}").bold()
            );
        }
        let wt = Worktree {
            path,
            branch: Some(branch.clone()),
        };
        (wt, true, Some(base_branch))
    };

    let mut vars = repo.worktree_vars(&wt);
    let from = repo.current_or_primary();
    vars.insert(
        "base".into(),
        base.or_else(|| from.branch.clone()).unwrap_or_default(),
    );
    vars.insert("base_worktree_path".into(), display(&from.path));
    vars.insert("target".into(), branch.clone());
    vars.insert("target_worktree_path".into(), display(&wt.path));

    if created && hooks {
        let config = HookConfigs::load(&wt.path);
        config.run_blocking(
            &repo,
            &["pre-start", "post-create"],
            &wt.path,
            &vars,
            opts.yes,
        )?;
        config.spawn_background(&repo, "post-start", &wt.path, &vars, opts.yes)?;
    }
    if hooks {
        HookConfigs::load(&wt.path).spawn_background(
            &repo,
            "post-switch",
            &wt.path,
            &vars,
            opts.yes,
        )?;
    }

    if !repo.is_current(&wt) {
        repo.remember_previous();
    }
    if !created {
        info(&format!(
            "Switched to worktree for {} @ {}",
            branch.bold(),
            tildify(&wt.path)
        ));
    }

    if opts.print_path {
        print!("{}", display(&wt.path));
        return Ok(());
    }
    let cd = write_cd_file(&wt.path)?;
    if let Some(cmd) = opts.execute {
        let cmd = render(&cmd, &vars)?;
        let args = opts
            .args
            .iter()
            .map(|a| render(a, &vars))
            .collect::<Result<Vec<_>>>()?;
        let status = Command::new("sh")
            .arg("-c")
            .arg(format!("{cmd} \"$@\""))
            .arg("sh")
            .args(&args)
            .current_dir(&wt.path)
            .status()
            .with_context(|| format!("failed to run {cmd}"))?;
        if !status.success() {
            bail!("{cmd} exited with {status}");
        }
    } else if !cd && !repo.is_current(&wt) {
        // Without shell integration we can't change the parent shell's
        // directory, so open a subshell there (same as `p <project>`).
        crate::spawn_shell_in(&display(&wt.path))?;
    }
    Ok(())
}

/// Fuzzy-find a worktree (or a branch without one) in this project, styled
/// like the project picker. Returns the chosen branch.
fn pick(repo: &Repo, query: Option<&str>) -> Result<Option<String>> {
    if !io::stdin().is_terminal() {
        bail!("no branch given; pass a branch name or run interactively");
    }
    let mut lines = Vec::new();
    for wt in &repo.worktrees {
        let Some(branch) = &wt.branch else { continue };
        let marker = if repo.is_current(wt) {
            "@"
        } else if repo.is_primary(wt) {
            "^"
        } else {
            " "
        };
        lines.push(format!(
            "{} {}\t{}\t{branch}",
            marker.bright_magenta(),
            crate::style_name(branch),
            crate::style_path(&tildify(&wt.path))
        ));
    }
    let branches = git(
        &repo.primary.path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    for branch in branches.lines().filter(|b| repo.for_branch(b).is_none()) {
        lines.push(format!(
            "{} {branch}\t{}\t{branch}",
            "/".dimmed(),
            "(no worktree)".dimmed()
        ));
    }
    let prompt = format!("Select worktree ({})>", repo.name());
    Ok(crate::fuzzy_select(&prompt, &lines.join("\n"), query)?
        .and_then(|line| line.rsplit('\t').next().map(str::to_string)))
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

struct RemoveOpts {
    branches: Vec<String>,
    force: bool,
    force_delete: bool,
    no_delete_branch: bool,
    yes: bool,
    no_hooks: bool,
}

fn remove(opts: RemoveOpts) -> Result<()> {
    let repo = Repo::discover()?;
    let names = if opts.branches.is_empty() {
        vec!["@".to_string()]
    } else {
        opts.branches.clone()
    };
    let mut removed_current = false;
    for name in names {
        let branch = repo.resolve_name(&name)?;
        let Some(wt) = repo.for_branch(&branch).cloned() else {
            bail!("no worktree for branch {branch}");
        };
        if repo.is_primary(&wt) {
            bail!(
                "{branch} is checked out in the primary worktree, which can't be removed; use {} instead",
                "p worktree switch".bold()
            );
        }
        removed_current |= repo.is_current(&wt);
        remove_one(
            &repo,
            &wt,
            &repo.primary,
            RemoveFlags {
                force: opts.force,
                force_delete: opts.force_delete,
                keep_branch: opts.no_delete_branch,
                yes: opts.yes,
                hooks: !opts.no_hooks,
            },
        )?;
    }
    if removed_current {
        leave_to(&repo.primary)?;
    }
    Ok(())
}

struct RemoveFlags {
    force: bool,
    force_delete: bool,
    keep_branch: bool,
    yes: bool,
    hooks: bool,
}

fn remove_one(repo: &Repo, wt: &Worktree, dest: &Worktree, flags: RemoveFlags) -> Result<()> {
    let branch = wt.branch.clone().unwrap_or_default();
    let status = git(&wt.path, &["status", "--porcelain"])?;
    if !status.trim().is_empty() && !flags.force {
        bail!("worktree for {branch} has uncommitted changes; commit them or pass --force",);
    }

    let mut vars = repo.worktree_vars(wt);
    vars.insert("base".into(), branch.clone());
    vars.insert("target".into(), dest.branch.clone().unwrap_or_default());
    vars.insert("target_worktree_path".into(), display(&dest.path));
    let config = HookConfigs::load(&wt.path);
    if flags.hooks {
        config.run_blocking(repo, &["pre-remove"], &wt.path, &vars, flags.yes)?;
    }

    let path_str = display(&wt.path);
    let mut args = vec!["worktree", "remove", path_str.as_str()];
    if flags.force {
        args.push("--force");
    }
    git_quiet(&repo.primary.path, &args)?;

    let mut note = String::new();
    if !flags.keep_branch && !branch.is_empty() {
        let integrated = is_integrated(&repo.primary.path, &branch, &repo.default_branch);
        if integrated || flags.force_delete {
            git_quiet(&repo.primary.path, &["branch", "-D", &branch])?;
            note = " & branch".into();
        } else {
            warn(&format!(
                "Kept branch {} (not merged into {}; pass -D to delete it)",
                branch.bold(),
                repo.default_branch
            ));
        }
    }
    success(&format!(
        "Removed {} worktree{note} @ {}",
        branch.bold(),
        tildify(&wt.path)
    ));

    if flags.hooks {
        config.spawn_background(repo, "post-remove", &repo.primary.path, &vars, flags.yes)?;
    }
    Ok(())
}

/// True when every commit on `branch` is already in `target` (or the trees match).
fn is_integrated(dir: &Path, branch: &str, target: &str) -> bool {
    let ancestor = Command::new("git")
        .args(["merge-base", "--is-ancestor", branch, target])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success());
    if ancestor {
        return true;
    }
    // Squash merges leave a different commit with the same tree.
    let tree = |r: &str| git(dir, &["rev-parse", &format!("{r}^{{tree}}")]).ok();
    matches!((tree(branch), tree(target)), (Some(a), Some(b)) if a == b)
}

/// Move the shell to `dest` after the worktree it was in disappeared.
fn leave_to(dest: &Worktree) -> Result<()> {
    if write_cd_file(&dest.path)? {
        info(&format!(
            "Switched to worktree for {} @ {}",
            dest.branch.as_deref().unwrap_or("(detached)").bold(),
            tildify(&dest.path)
        ));
    } else {
        info(&format!(
            "Current directory was removed; run {}",
            format!("cd {}", display(&dest.path)).bold()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// merge
// ---------------------------------------------------------------------------

struct MergeOpts {
    target: Option<String>,
    no_squash: bool,
    no_remove: bool,
    message: Option<String>,
    yes: bool,
    no_hooks: bool,
}

fn merge(opts: MergeOpts) -> Result<()> {
    let repo = Repo::discover()?;
    let wt = repo
        .current
        .clone()
        .context("run p worktree merge from inside a worktree")?;
    let branch = wt
        .branch
        .clone()
        .context("current worktree is detached; check out a branch first")?;
    let target = match &opts.target {
        Some(t) => repo.resolve_name(t)?,
        None => repo.default_branch.clone(),
    };
    if branch == target {
        bail!("already on {target}; switch to a feature worktree to merge it");
    }
    ensure_branch(&wt.path, &target)?;
    let status = git(&wt.path, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        bail!("{branch} has uncommitted changes; commit them before merging");
    }

    let count: u64 = git(
        &wt.path,
        &["rev-list", "--count", &format!("{target}..HEAD")],
    )?
    .parse()
    .unwrap_or(0);
    if count == 0 {
        bail!(
            "{branch} has no commits beyond {target}; remove it with {}",
            "p worktree remove".bold()
        );
    }

    // Rebase onto the target so the merge is a fast-forward.
    if !is_ancestor(&wt.path, &target, "HEAD") {
        progress(&format!(
            "Rebasing {} onto {}",
            branch.bold(),
            target.bold()
        ));
        if git_quiet(&wt.path, &["rebase", &target]).is_err() {
            let _ = git(&wt.path, &["rebase", "--abort"]);
            bail!(
                "rebase onto {target} hit conflicts; resolve them manually (git rebase {target})"
            );
        }
    }

    if count > 1 && !opts.no_squash {
        let message = match &opts.message {
            Some(m) => m.clone(),
            None => squash_message(&wt.path, &target, &branch)?,
        };
        progress(&format!("Squashing {count} commits"));
        git_quiet(&wt.path, &["reset", "--soft", &target])?;
        git_quiet(&wt.path, &["commit", "--quiet", "-m", &message])?;
    }

    let target_wt = repo.for_branch(&target).cloned();
    let mut vars = repo.worktree_vars(&Worktree {
        path: wt.path.clone(),
        branch: Some(branch.clone()),
    });
    vars.insert("base".into(), branch.clone());
    vars.insert("target".into(), target.clone());
    if let Some(t) = &target_wt {
        vars.insert("target_worktree_path".into(), display(&t.path));
    }
    let hooks = !opts.no_hooks;
    let config = HookConfigs::load(&wt.path);
    if hooks {
        config.run_blocking(&repo, &["pre-merge"], &wt.path, &vars, opts.yes)?;
    }

    // Fast-forward the target branch.
    let new_head = git(&wt.path, &["rev-parse", "HEAD"])?;
    let old_target = git(&wt.path, &["rev-parse", &format!("refs/heads/{target}")])?;
    match &target_wt {
        Some(t) => {
            ensure_clean_tree(&t.path, &format!("{target} worktree"))?;
            git_quiet(&t.path, &["merge", "--ff-only", "--quiet", &new_head])?;
        }
        None => {
            git_quiet(
                &wt.path,
                &[
                    "update-ref",
                    &format!("refs/heads/{target}"),
                    &new_head,
                    &old_target,
                ],
            )?;
        }
    }
    let short = git(&wt.path, &["rev-parse", "--short", "HEAD"])?;
    success(&format!(
        "Merged {} to {} @ {short}",
        branch.bold(),
        target.bold()
    ));

    let dest = target_wt.clone().unwrap_or_else(|| repo.primary.clone());
    if hooks {
        config.spawn_background(&repo, "post-merge", &dest.path, &vars, opts.yes)?;
    }

    if opts.no_remove {
        return Ok(());
    }
    if repo.is_primary(&wt) {
        if target_wt.is_none() {
            git_quiet(&wt.path, &["switch", "--quiet", &target])?;
            git_quiet(&wt.path, &["branch", "-D", &branch])?;
            success(&format!(
                "Switched primary worktree to {} and deleted {}",
                target.bold(),
                branch.bold()
            ));
        }
        return Ok(());
    }
    remove_one(
        &repo,
        &wt,
        &dest,
        RemoveFlags {
            force: false,
            force_delete: false,
            keep_branch: false,
            yes: opts.yes,
            hooks,
        },
    )?;
    leave_to(&dest)
}

fn squash_message(dir: &Path, target: &str, branch: &str) -> Result<String> {
    let subjects = git(
        dir,
        &[
            "log",
            "--reverse",
            "--format=%s",
            &format!("{target}..HEAD"),
        ],
    )?;
    let mut message = format!("Merge {branch}\n\n");
    for subject in subjects.lines() {
        message.push_str(&format!("- {subject}\n"));
    }
    Ok(message)
}

// ---------------------------------------------------------------------------
// checkout (pre-existing): move a linked worktree's branch onto the primary
// ---------------------------------------------------------------------------

fn checkout(branch: &str) -> Result<()> {
    let repo = Repo::discover()?;
    let primary = &repo.primary;
    ensure_branch(&primary.path, branch)?;
    ensure_clean_tree(&primary.path, "primary worktree")?;
    let source = repo.for_branch(branch);

    let mut stash_ref = None;
    if let Some(source) = source.filter(|w| w.path != primary.path) {
        stash_ref = stash_worktree_changes(&source.path, branch)?;
        git_run(&source.path, &["switch", "--detach"])?;
    }

    git_run(&primary.path, &["switch", branch])?;

    if let Some(stash_ref) = stash_ref {
        git_run(&primary.path, &["stash", "apply", "--index", &stash_ref])?;
        drop_stash_if_still_top(&primary.path, &stash_ref)?;
    }

    success(&format!(
        "Checked out {} in {}",
        branch.bold(),
        tildify(&primary.path)
    ));
    Ok(())
}

fn stash_worktree_changes(path: &Path, branch: &str) -> Result<Option<String>> {
    let status = git(path, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(None);
    }
    let before = git(path, &["rev-parse", "--verify", "-q", "refs/stash"]).ok();
    git_run(
        path,
        &[
            "stash",
            "push",
            "-u",
            "-m",
            &format!("pm worktree checkout {branch}"),
        ],
    )?;
    let after = git(path, &["rev-parse", "--verify", "-q", "stash@{0}"]).ok();
    if before == after {
        bail!("git stash did not create a stash for {}", path.display());
    }
    Ok(after)
}

fn drop_stash_if_still_top(path: &Path, stash_hash: &str) -> Result<()> {
    let top = git(path, &["rev-parse", "--verify", "-q", "stash@{0}"])?;
    if top.trim() != stash_hash.trim() {
        bail!("applied stash {stash_hash}, but it is no longer stash@{{0}}; leaving it in place");
    }
    git_run(path, &["stash", "drop", "stash@{0}"])
}

// ---------------------------------------------------------------------------
// Hooks (worktrunk-compatible)
// ---------------------------------------------------------------------------

/// One hook definition: a list of steps run in order; commands within a step
/// run concurrently. Mirrors worktrunk's string / table / `[[pipeline]]` forms.
type Pipeline = Vec<Vec<(Option<String>, String)>>;

#[derive(Default)]
struct UserConfig {
    worktree_path: Option<String>,
    hooks: toml::Table,
}

impl UserConfig {
    fn path() -> Option<PathBuf> {
        if let Ok(p) = env::var("WORKTRUNK_CONFIG_PATH") {
            return Some(PathBuf::from(p));
        }
        let base = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))?;
        Some(base.join("worktrunk").join("config.toml"))
    }

    fn load() -> Self {
        let Some(table) = Self::path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| s.parse::<toml::Table>().ok())
        else {
            return Self::default();
        };
        Self {
            worktree_path: table
                .get("worktree-path")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            hooks: table,
        }
    }
}

struct HookConfigs {
    user: toml::Table,
    project: toml::Table,
}

#[derive(Clone, Copy, PartialEq)]
enum Source {
    User,
    Project,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::User => "user",
            Source::Project => "project",
        }
    }
}

impl HookConfigs {
    fn load(worktree: &Path) -> Self {
        let project = fs::read_to_string(worktree.join(".config").join("wt.toml"))
            .ok()
            .and_then(|s| match s.parse::<toml::Table>() {
                Ok(t) => Some(t),
                Err(e) => {
                    warn(&format!("ignoring invalid .config/wt.toml: {e}"));
                    None
                }
            })
            .unwrap_or_default();
        Self {
            user: UserConfig::load().hooks,
            project,
        }
    }

    fn pipelines(&self, hook: &str) -> Vec<(Source, Pipeline)> {
        let mut out = Vec::new();
        for (source, table) in [(Source::User, &self.user), (Source::Project, &self.project)] {
            if let Some(value) = table.get(hook)
                && let Some(p) = parse_pipeline(value)
            {
                out.push((source, p));
            }
        }
        out
    }

    /// Run `pre-*` hooks to completion; any failure aborts the operation.
    fn run_blocking(
        &self,
        repo: &Repo,
        hook_types: &[&str],
        cwd: &Path,
        vars: &Vars,
        yes: bool,
    ) -> Result<()> {
        for hook in hook_types {
            for (source, pipeline) in self.pipelines(hook) {
                if source == Source::Project && !approve(repo, hook, &pipeline, yes)? {
                    continue;
                }
                for step in &pipeline {
                    let mut children: Vec<(String, Child)> = Vec::new();
                    for (name, template) in step {
                        let label = hook_label(hook, source, name.as_deref());
                        let cmd = render(template, &hook_vars(vars, hook, name, cwd))
                            .with_context(|| format!("{label}: bad template"))?;
                        progress(&format!("Running {label}"));
                        eprintln!("  {}", cmd.dimmed());
                        let child = Command::new("sh")
                            .arg("-c")
                            .arg(&cmd)
                            .current_dir(cwd)
                            .stdin(Stdio::null())
                            .spawn()
                            .with_context(|| format!("failed to start {label}"))?;
                        children.push((label, child));
                    }
                    for (label, mut child) in children {
                        let status = child.wait()?;
                        if !status.success() {
                            bail!("{label} failed ({status})");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Start `post-*` hooks in the background, logging to `.git/pm-logs/`.
    fn spawn_background(
        &self,
        repo: &Repo,
        hook: &str,
        cwd: &Path,
        vars: &Vars,
        yes: bool,
    ) -> Result<()> {
        for (source, pipeline) in self.pipelines(hook) {
            if source == Source::Project && !approve(repo, hook, &pipeline, yes)? {
                continue;
            }
            // Render every step up front, then hand the pipeline to one shell so
            // steps stay ordered while the whole thing runs detached.
            let mut script = String::from("set -e\npids=\n");
            let mut names = Vec::new();
            for step in &pipeline {
                for (name, template) in step {
                    let cmd = render(template, &hook_vars(vars, hook, name, cwd))?;
                    script.push_str(&format!("( {cmd}\n) &\npids=\"$pids $!\"\n"));
                    names.push(name.clone().unwrap_or_else(|| "cmd".into()));
                }
                script.push_str("for pid in $pids; do wait \"$pid\"; done\npids=\n");
            }
            let dir = repo.log_dir();
            fs::create_dir_all(&dir)?;
            let branch = vars.get("branch").map(|b| sanitize(b)).unwrap_or_default();
            let log_path = dir.join(format!("{branch}-{}-{hook}.log", source.label()));
            let log = fs::File::create(&log_path)?;
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg(&script)
                .current_dir(cwd)
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log);
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            command
                .spawn()
                .with_context(|| format!("failed to start {hook} hook"))?;
            info(&format!(
                "Started {} {hook} ({}) in background; log @ {}",
                source.label(),
                names.join(", "),
                tildify(&log_path)
            ));
        }
        Ok(())
    }
}

fn hook_label(hook: &str, source: Source, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{hook} {}:{n}", source.label()),
        None => format!("{hook} {}", source.label()),
    }
}

fn hook_vars(vars: &Vars, hook: &str, name: &Option<String>, cwd: &Path) -> Vars {
    let mut v = vars.clone();
    v.insert("hook_type".into(), hook.to_string());
    v.insert("cwd".into(), display(cwd));
    if let Some(n) = name {
        v.insert("hook_name".into(), n.clone());
    }
    v
}

fn parse_pipeline(value: &toml::Value) -> Option<Pipeline> {
    fn table_step(t: &toml::Table) -> Vec<(Option<String>, String)> {
        t.iter()
            .filter_map(|(k, v)| v.as_str().map(|c| (Some(k.clone()), c.to_string())))
            .collect()
    }
    match value {
        toml::Value::String(cmd) => Some(vec![vec![(None, cmd.clone())]]),
        toml::Value::Table(t) => Some(vec![table_step(t)]),
        toml::Value::Array(steps) => Some(
            steps
                .iter()
                .filter_map(|s| match s {
                    toml::Value::Table(t) => Some(table_step(t)),
                    toml::Value::String(cmd) => Some(vec![(None, cmd.clone())]),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// Project hooks come from the repository, so (like worktrunk) they need
/// approval before they run. Approvals are remembered in the repo's local git
/// config, which a clone can't pre-populate.
fn approve(repo: &Repo, hook: &str, pipeline: &Pipeline, yes: bool) -> Result<bool> {
    let commands: Vec<&String> = pipeline.iter().flatten().map(|(_, c)| c).collect();
    let key = format!(
        "{hook}:{:016x}",
        fnv1a(
            commands
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .as_bytes()
        )
    );
    let approved = git(
        &repo.primary.path,
        &["config", "--get-all", APPROVED_HOOK_KEY],
    )
    .unwrap_or_default();
    if approved.lines().any(|l| l == key) {
        return Ok(true);
    }
    if !yes {
        if !io::stdin().is_terminal() {
            warn(&format!(
                "Skipping unapproved project {hook} hook from .config/wt.toml (rerun with --yes)"
            ));
            return Ok(false);
        }
        eprintln!(
            "{} .config/wt.toml wants to run {} hook:",
            "▲".yellow(),
            hook.bold()
        );
        for cmd in &commands {
            eprintln!("  {}", cmd.dimmed());
        }
        eprint!("Allow and remember? [y/N] ");
        io::stderr().flush()?;
        let mut answer = String::new();
        io::stdin().lock().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            return Ok(false);
        }
    }
    git(
        &repo.primary.path,
        &["config", "--local", "--add", APPROVED_HOOK_KEY, &key],
    )?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Templates: the subset of worktrunk's minijinja syntax that matters here.
// ---------------------------------------------------------------------------

type Vars = BTreeMap<String, String>;

pub fn render(template: &str, vars: &Vars) -> Result<String> {
    if template.contains("{%") {
        bail!("{{% ... %}} blocks are not supported by p; simplify the template: {template}");
    }
    let re = Regex::new(r"\{\{\s*([^}]*?)\s*\}\}").expect("valid regex");
    let mut out = String::new();
    let mut last = 0;
    for cap in re.captures_iter(template) {
        let whole = cap.get(0).expect("match");
        out.push_str(&template[last..whole.start()]);
        let mut parts = cap[1].split('|').map(str::trim);
        let var = parts.next().unwrap_or_default();
        let mut value = vars
            .get(var)
            .cloned()
            .with_context(|| format!("undefined template variable: {var}"))?;
        for filter in parts {
            value = match filter {
                "sanitize" => sanitize(&value),
                "basename" => file_name(Path::new(&value)),
                "dirname" => Path::new(&value).parent().map(display).unwrap_or_default(),
                "hash_port" => (10_000 + fnv1a(value.as_bytes()) % 10_000).to_string(),
                "lower" => value.to_lowercase(),
                "upper" => value.to_uppercase(),
                other => bail!("unsupported template filter: {other}"),
            };
        }
        out.push_str(&value);
        last = whole.end();
    }
    out.push_str(&template[last..]);
    Ok(out)
}

pub fn sanitize(s: &str) -> String {
    s.replace(['/', '\\'], "-")
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}

// ---------------------------------------------------------------------------
// Shell integration, output, and git helpers
// ---------------------------------------------------------------------------

/// Ask the wrapping shell function to `cd` into `path`. Returns false when the
/// shell integration isn't installed.
fn write_cd_file(path: &Path) -> Result<bool> {
    let Some(file) = env::var_os(CD_FILE_ENV) else {
        return Ok(false);
    };
    fs::write(&file, display(path)).context("failed to write cd directive")?;
    Ok(true)
}

fn success(msg: &str) {
    eprintln!("{} {msg}", "✓".green().bold());
}

fn progress(msg: &str) {
    eprintln!("{} {msg}", "◎".cyan().bold());
}

fn info(msg: &str) {
    eprintln!("{} {msg}", "○".dimmed());
}

fn warn(msg: &str) {
    eprintln!("{} {msg}", "▲".yellow().bold());
}

fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_string())
}

/// Run git, surfacing its stderr only on failure.
fn git_quiet(cwd: &Path, args: &[&str]) -> Result<()> {
    git(cwd, args).map(|_| ())
}

/// Run git with inherited stdio.
fn git_run(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .context("failed to run git")?;
    if !status.success() {
        bail!("git command failed in {}", cwd.display());
    }
    Ok(())
}

fn branch_exists(dir: &Path, branch: &str) -> bool {
    git(
        dir,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

fn remote_branch_exists(dir: &Path, branch: &str) -> bool {
    git(
        dir,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )
    .is_ok()
}

fn ensure_branch(dir: &Path, branch: &str) -> Result<()> {
    if branch_exists(dir, branch) {
        Ok(())
    } else {
        bail!("branch not found: {branch}")
    }
}

fn ensure_clean_tree(path: &Path, label: &str) -> Result<()> {
    let status = git(path, &["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        bail!(
            "{label} has uncommitted changes; commit, stash, or clean {} first",
            path.display()
        );
    }
    Ok(())
}

fn is_ancestor(dir: &Path, ancestor: &str, rev: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, rev])
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(dirs) = directories::BaseDirs::new()
    {
        return dirs.home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn tildify(path: &Path) -> String {
    if let Some(dirs) = directories::BaseDirs::new()
        && let Ok(rest) = path.strip_prefix(dirs.home_dir())
    {
        return format!("~/{}", rest.display());
    }
    display(path)
}

/// Lexically resolve `.` and `..` (the target directory may not exist yet).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars::from([
            ("repo".to_string(), "pm".to_string()),
            ("repo_path".to_string(), "/code/pm".to_string()),
            ("branch".to_string(), "feat/auth".to_string()),
        ])
    }

    #[test]
    fn default_worktree_path_is_sibling_of_repo() {
        let rendered = render(DEFAULT_WORKTREE_PATH, &vars()).unwrap();
        assert_eq!(
            normalize(Path::new(&rendered)),
            PathBuf::from("/code/pm.feat-auth")
        );
    }

    #[test]
    fn render_supports_filters_and_rejects_unknowns() {
        let v = vars();
        assert_eq!(render("{{repo_path|basename}}", &v).unwrap(), "pm");
        assert_eq!(render("{{ repo_path | dirname }}", &v).unwrap(), "/code");
        let port: u32 = render("{{ branch | hash_port }}", &v)
            .unwrap()
            .parse()
            .unwrap();
        assert!((10_000..20_000).contains(&port));
        assert!(render("{{ nope }}", &v).is_err());
        assert!(render("{{ branch | wat }}", &v).is_err());
        assert!(render("{% if branch %}x{% endif %}", &v).is_err());
    }

    #[test]
    fn parses_porcelain_worktrees() {
        let porcelain = "worktree /code/pm\nHEAD abc\nbranch refs/heads/main\n\nworktree /code/pm.feat\nHEAD def\ndetached\n";
        let wts = parse_worktrees(porcelain);
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[1].branch, None);
    }

    #[test]
    fn parses_status_flags() {
        assert_eq!(parse_status("M  a\n?? b\n"), (true, false, true));
        assert_eq!(parse_status(" M a\n"), (false, true, false));
        assert_eq!(parse_status(""), (false, false, false));
    }

    #[test]
    fn parses_all_three_hook_forms() {
        let t: toml::Table = r#"
            pre-start = "npm ci"
            [post-start]
            server = "npm run dev"
            [[pre-merge]]
            install = "npm ci"
            [[pre-merge]]
            test = "npm test"
            lint = "npm run lint"
        "#
        .parse()
        .unwrap();
        assert_eq!(
            parse_pipeline(&t["pre-start"]).unwrap(),
            vec![vec![(None, "npm ci".to_string())]]
        );
        assert_eq!(parse_pipeline(&t["post-start"]).unwrap()[0].len(), 1);
        let merge = parse_pipeline(&t["pre-merge"]).unwrap();
        assert_eq!(merge.len(), 2);
        assert_eq!(merge[1].len(), 2);
    }

    #[test]
    fn parses_remote_owner_and_name() {
        assert_eq!(
            parse_remote("git@github.com:jakeisnt/pm.git"),
            Some(("jakeisnt".into(), "pm".into()))
        );
        assert_eq!(
            parse_remote("https://github.com/jakeisnt/pm"),
            Some(("jakeisnt".into(), "pm".into()))
        );
    }

    #[test]
    fn formats_ages() {
        assert_eq!(age(5), "5s");
        assert_eq!(age(7200), "2h");
        assert_eq!(age(3 * 86_400), "3d");
    }
}
