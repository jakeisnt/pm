//! Worktree management modelled on worktrunk (https://worktrunk.dev): worktrees
//! are addressed by branch name and their paths come from a template.

mod hooks;
mod template;

use crate::{
    cli::{HookArgs, ListArgs, MergeArgs, RemoveArgs, SwitchArgs, WorktreeCmd},
    git, ui,
    ui::{info, progress, success, tildify, warn},
};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use hooks::Hooks;
use serde::Serialize;
use std::{
    env, fs,
    io::{self, IsTerminal},
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use template::{Vars, render};

pub fn run(cmd: Option<WorktreeCmd>) -> Result<()> {
    match cmd.unwrap_or(WorktreeCmd::List(ListArgs::default())) {
        WorktreeCmd::Switch(args) => switch(args),
        WorktreeCmd::List(args) => list(args),
        WorktreeCmd::Remove(args) => remove(args),
        WorktreeCmd::Merge(args) => merge(args),
        WorktreeCmd::Checkout { branch_name } => checkout(&branch_name),
    }
}

/// When set (by the shell integration from `p hook-install`), commands that
/// change worktrees write the destination path here and the shell `cd`s to it.
const CD_FILE_ENV: &str = "PM_CD_FILE";
const DEFAULT_WORKTREE_PATH: &str = "{{ repo_path }}/../{{ repo }}.{{ branch | sanitize }}";
const PREVIOUS_BRANCH_KEY: &str = "pm.previousBranch";
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
        let worktrees = parse_worktrees(&git::output(&cwd, &["worktree", "list", "--porcelain"])?);
        if worktrees.is_empty() {
            bail!("no git worktrees found");
        }
        let common_dir = PathBuf::from(git::output(
            &cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?);
        let primary = worktrees
            .iter()
            .find(|w| Some(w.path.as_path()) == common_dir.parent())
            .or_else(|| worktrees.first())
            .cloned()
            .context("could not determine primary worktree")?;
        let top = git::output(&cwd, &["rev-parse", "--show-toplevel"])
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
            "-" => git::output(
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
            let _ = git::output(
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
        if let Ok(url) = git::output(&self.primary.path, &["remote", "get-url", "origin"]) {
            v.insert("remote".into(), "origin".into());
            if let Some((owner, name)) = git::parse_github_remote(&url)
                .as_deref()
                .and_then(|full| full.split_once('/'))
            {
                v.insert("owner".into(), owner.to_string());
                v.insert("remote_repo".into(), name.to_string());
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
            if let Ok(up) = git::output(
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
        if let Ok(sha) = git::output(&wt.path, &["rev-parse", "HEAD"]) {
            v.insert("commit".into(), sha);
        }
        if let Ok(sha) = git::output(&wt.path, &["rev-parse", "--short", "HEAD"]) {
            v.insert("short_commit".into(), sha);
        }
        v
    }

    fn path_for(&self, branch: &str) -> Result<PathBuf> {
        let template =
            hooks::worktree_path_template().unwrap_or_else(|| DEFAULT_WORKTREE_PATH.to_string());
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

    /// Hooks configured for `worktree` (project config is read from there).
    fn hooks(&self, worktree: &Path, opts: HookArgs) -> Hooks {
        Hooks::load(
            worktree,
            &self.primary.path,
            self.common_dir.join("pm-logs"),
            opts,
        )
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
    if let Ok(head) = git::output(
        path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && let Some(branch) = head.strip_prefix("origin/")
        && git::branch_exists(path, branch)
    {
        return branch.to_string();
    }
    if let Ok(configured) = git::output(path, &["config", "--get", "init.defaultBranch"])
        && git::branch_exists(path, &configured)
    {
        return configured;
    }
    for candidate in ["main", "master", "trunk", "develop"] {
        if git::branch_exists(path, candidate) {
            return candidate.to_string();
        }
    }
    primary.branch.clone().unwrap_or_else(|| "main".to_string())
}

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

fn list(
    ListArgs {
        json,
        branches: include_branches,
    }: ListArgs,
) -> Result<()> {
    let repo = Repo::discover()?;
    let mut rows: Vec<ListRow> = repo
        .worktrees
        .iter()
        .map(|wt| row_for(&repo, Some(wt), wt.branch.as_deref()))
        .collect();
    if include_branches {
        let branches = git::output(
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
        if let Ok(status) = git::output(&wt.path, &["status", "--porcelain"]) {
            (staged, modified, untracked) = parse_status(&status);
        }
        if let Ok(numstat) = git::output(&wt.path, &["diff", "HEAD", "--numstat"]) {
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
    let log =
        git::output(dir, &["log", "-1", "--format=%h%x09%ct%x09%s", &rev]).unwrap_or_default();
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
    let out = git::output(
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

fn switch(opts: SwitchArgs) -> Result<()> {
    let repo = Repo::discover()?;
    let branch = match &opts.branch {
        Some(name) => {
            let branch = repo.resolve_name(name)?;
            let known = repo.for_branch(&branch).is_some()
                || git::branch_exists(&repo.primary.path, &branch)
                || git::remote_branch_exists(&repo.primary.path, &branch);
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
            if git::branch_exists(&repo.primary.path, &branch) {
                bail!("branch {branch} already exists; switch to it without --create");
            }
            base_branch = match &opts.base {
                Some(b) => repo.resolve_name(b)?,
                None => repo.default_branch.clone(),
            };
            git::output(
                &repo.primary.path,
                &["worktree", "add", "-b", &branch, &path_str, &base_branch],
            )?;
            success(&format!(
                "Created branch {} from {} and worktree @ {}",
                branch.bold(),
                base_branch.bold(),
                tildify(&path)
            ));
        } else if git::branch_exists(&repo.primary.path, &branch)
            || git::remote_branch_exists(&repo.primary.path, &branch)
        {
            if opts.base.is_some() {
                bail!("--base only applies with --create");
            }
            base_branch = repo.current_or_primary().branch.clone().unwrap_or_default();
            // git DWIMs `origin/<branch>` into a tracking branch when no local one exists.
            git::output(&repo.primary.path, &["worktree", "add", &path_str, &branch])?;
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

    let hooks = repo.hooks(&wt.path, opts.hooks);
    if created {
        hooks.run(&["pre-start", "post-create"], &wt.path, &vars)?;
        hooks.spawn("post-start", &wt.path, &vars)?;
    }
    hooks.spawn("post-switch", &wt.path, &vars)?;

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
        ui::spawn_shell_in(&display(&wt.path))?;
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
            ui::style_name(branch),
            ui::style_path(&tildify(&wt.path))
        ));
    }
    let branches = git::output(
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
    Ok(ui::fuzzy_select(&prompt, &lines.join("\n"), query)?
        .and_then(|line| line.rsplit('\t').next().map(str::to_string)))
}

fn remove(opts: RemoveArgs) -> Result<()> {
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
        remove_one(&repo, &wt, &repo.primary, &opts)?;
    }
    if removed_current {
        leave_to(&repo.primary)?;
    }
    Ok(())
}

/// Remove `wt` (moving the user to `dest`), deleting its branch once merged.
fn remove_one(repo: &Repo, wt: &Worktree, dest: &Worktree, opts: &RemoveArgs) -> Result<()> {
    let branch = wt.branch.clone().unwrap_or_default();
    if !opts.force && !git::is_clean(&wt.path)? {
        bail!("worktree for {branch} has uncommitted changes; commit them or pass --force",);
    }

    let mut vars = repo.worktree_vars(wt);
    vars.insert("base".into(), branch.clone());
    vars.insert("target".into(), dest.branch.clone().unwrap_or_default());
    vars.insert("target_worktree_path".into(), display(&dest.path));
    let hooks = repo.hooks(&wt.path, opts.hooks);
    hooks.run(&["pre-remove"], &wt.path, &vars)?;

    let path_str = display(&wt.path);
    let mut args = vec!["worktree", "remove", path_str.as_str()];
    if opts.force {
        args.push("--force");
    }
    git::output(&repo.primary.path, &args)?;

    let mut note = String::new();
    if !opts.no_delete_branch && !branch.is_empty() {
        let integrated = is_integrated(&repo.primary.path, &branch, &repo.default_branch);
        if integrated || opts.force_delete {
            git::output(&repo.primary.path, &["branch", "-D", &branch])?;
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

    hooks.spawn("post-remove", &repo.primary.path, &vars)
}

/// True when every commit on `branch` is already in `target` (or the trees match).
fn is_integrated(dir: &Path, branch: &str, target: &str) -> bool {
    if git::is_ancestor(dir, branch, target) {
        return true;
    }
    // Squash merges leave a different commit with the same tree.
    let tree = |r: &str| git::output(dir, &["rev-parse", &format!("{r}^{{tree}}")]).ok();
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

fn merge(opts: MergeArgs) -> Result<()> {
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
    if !git::branch_exists(&wt.path, &target) {
        bail!("branch not found: {target}");
    }
    git::ensure_clean(&wt.path, &branch)?;

    let count: u64 = git::output(
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

    // Only empty commits (or ones that cancel out): nothing to merge.
    if git::succeeds(&wt.path, &["diff", "--quiet", &target, "HEAD"]) {
        bail!(
            "{branch} has no changes compared to {target}; remove it with {}",
            "p worktree remove".bold()
        );
    }

    // Rebase onto the target so the merge is a fast-forward.
    if !git::is_ancestor(&wt.path, &target, "HEAD") {
        progress(&format!(
            "Rebasing {} onto {}",
            branch.bold(),
            target.bold()
        ));
        if git::output(&wt.path, &["rebase", &target]).is_err() {
            let _ = git::output(&wt.path, &["rebase", "--abort"]);
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
        let original = git::output(&wt.path, &["rev-parse", "HEAD"])?;
        git::output(&wt.path, &["reset", "--soft", &target])?;
        if let Err(e) = git::output(&wt.path, &["commit", "--quiet", "-m", &message]) {
            // e.g. a pre-commit hook said no: put the branch back as it was.
            git::output(&wt.path, &["reset", "--soft", &original])?;
            return Err(e.context(format!("squash failed; {branch} is unchanged")));
        }
    }

    let target_wt = repo.for_branch(&target).cloned();
    let mut vars = repo.worktree_vars(&wt);
    vars.insert("base".into(), branch.clone());
    vars.insert("target".into(), target.clone());
    if let Some(t) = &target_wt {
        vars.insert("target_worktree_path".into(), display(&t.path));
    }
    let hooks = repo.hooks(&wt.path, opts.hooks);
    hooks.run(&["pre-merge"], &wt.path, &vars)?;

    // Fast-forward the target branch.
    let new_head = git::output(&wt.path, &["rev-parse", "HEAD"])?;
    let old_target = git::output(&wt.path, &["rev-parse", &format!("refs/heads/{target}")])?;
    match &target_wt {
        Some(t) => {
            git::ensure_clean(&t.path, &format!("{target} worktree"))?;
            git::output(&t.path, &["merge", "--ff-only", "--quiet", &new_head])?;
        }
        None => {
            git::output(
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
    let short = git::output(&wt.path, &["rev-parse", "--short", "HEAD"])?;
    success(&format!(
        "Merged {} to {} @ {short}",
        branch.bold(),
        target.bold()
    ));

    let dest = target_wt.clone().unwrap_or_else(|| repo.primary.clone());
    hooks.spawn("post-merge", &dest.path, &vars)?;

    if opts.no_remove {
        return Ok(());
    }
    if repo.is_primary(&wt) {
        if target_wt.is_none() {
            git::output(&wt.path, &["switch", "--quiet", &target])?;
            git::output(&wt.path, &["branch", "-D", &branch])?;
            success(&format!(
                "Switched primary worktree to {} and deleted {}",
                target.bold(),
                branch.bold()
            ));
        }
        return Ok(());
    }
    let remove_opts = RemoveArgs {
        branches: Vec::new(),
        force: false,
        force_delete: false,
        no_delete_branch: false,
        hooks: opts.hooks,
    };
    remove_one(&repo, &wt, &dest, &remove_opts)?;
    leave_to(&dest)
}

fn squash_message(dir: &Path, target: &str, branch: &str) -> Result<String> {
    let subjects = git::output(
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

fn checkout(branch: &str) -> Result<()> {
    let repo = Repo::discover()?;
    let primary = &repo.primary;
    if !git::branch_exists(&primary.path, branch) {
        bail!("branch not found: {branch}");
    }
    git::ensure_clean(&primary.path, "primary worktree")?;
    let source = repo.for_branch(branch);

    let mut stash_ref = None;
    if let Some(source) = source.filter(|w| w.path != primary.path) {
        stash_ref = stash_worktree_changes(&source.path, branch)?;
        git::run(&source.path, &["switch", "--detach"])?;
    }

    git::run(&primary.path, &["switch", branch])?;

    if let Some(stash_ref) = stash_ref {
        git::run(&primary.path, &["stash", "apply", "--index", &stash_ref])?;
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
    let status = git::output(path, &["status", "--porcelain"])?;
    if status.trim().is_empty() {
        return Ok(None);
    }
    let before = git::output(path, &["rev-parse", "--verify", "-q", "refs/stash"]).ok();
    git::run(
        path,
        &[
            "stash",
            "push",
            "-u",
            "-m",
            &format!("pm worktree checkout {branch}"),
        ],
    )?;
    let after = git::output(path, &["rev-parse", "--verify", "-q", "stash@{0}"]).ok();
    if before == after {
        bail!("git stash did not create a stash for {}", path.display());
    }
    Ok(after)
}

fn drop_stash_if_still_top(path: &Path, stash_hash: &str) -> Result<()> {
    let top = git::output(path, &["rev-parse", "--verify", "-q", "stash@{0}"])?;
    if top.trim() != stash_hash.trim() {
        bail!("applied stash {stash_hash}, but it is no longer stash@{{0}}; leaving it in place");
    }
    git::run(path, &["stash", "drop", "stash@{0}"])
}

/// Ask the wrapping shell function to `cd` into `path`. Returns false when the
/// shell integration isn't installed.
fn write_cd_file(path: &Path) -> Result<bool> {
    let Some(file) = env::var_os(CD_FILE_ENV) else {
        return Ok(false);
    };
    fs::write(&file, display(path)).context("failed to write cd directive")?;
    Ok(true)
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

    #[test]
    fn default_worktree_path_is_sibling_of_repo() {
        let vars = Vars::from([
            ("repo".to_string(), "pm".to_string()),
            ("repo_path".to_string(), "/code/pm".to_string()),
            ("branch".to_string(), "feat/auth".to_string()),
        ]);
        let rendered = render(DEFAULT_WORKTREE_PATH, &vars).unwrap();
        assert_eq!(
            normalize(Path::new(&rendered)),
            PathBuf::from("/code/pm.feat-auth")
        );
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
    fn formats_ages() {
        assert_eq!(age(5), "5s");
        assert_eq!(age(7200), "2h");
        assert_eq!(age(3 * 86_400), "3d");
    }
}
