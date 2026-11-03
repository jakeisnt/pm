//! worktrunk-compatible configuration and hooks.
//!
//! - User config: `~/.config/worktrunk/config.toml` (`worktree-path`, user hooks)
//! - Project config: `.config/wt.toml` in the worktree (project hooks)
//!
//! A hook is a string, a table of named commands (run concurrently), or an
//! array of such tables (run in order). `pre-*` hooks block and abort the
//! operation on failure; `post-*` hooks run detached and log to `.git/pm-logs/`.

use super::template::{self, Vars};
use crate::{cli::HookArgs, git, ui};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

const APPROVED_HOOK_KEY: &str = "pm.approvedHook";

/// Steps run in order; the named commands within a step run concurrently.
type Pipeline = Vec<Vec<(Option<String>, String)>>;

fn user_config() -> toml::Table {
    let path = env::var_os("WORKTRUNK_CONFIG_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".config")))
                .map(|base| base.join("worktrunk").join("config.toml"))
        });
    path.and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or_default()
}

/// The user's `worktree-path` template, if set.
pub fn worktree_path_template() -> Option<String> {
    user_config()
        .get("worktree-path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
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

pub struct Hooks {
    user: toml::Table,
    project: toml::Table,
    opts: HookArgs,
    /// Where approvals are stored (the primary worktree).
    repo_path: PathBuf,
    log_dir: PathBuf,
}

impl Hooks {
    pub fn load(worktree: &Path, repo_path: &Path, log_dir: PathBuf, opts: HookArgs) -> Self {
        let project = match fs::read_to_string(worktree.join(".config").join("wt.toml")) {
            Ok(text) => text.parse().unwrap_or_else(|e| {
                ui::warn(&format!("ignoring invalid .config/wt.toml: {e}"));
                toml::Table::new()
            }),
            Err(_) => toml::Table::new(),
        };
        Self {
            user: user_config(),
            project,
            opts,
            repo_path: repo_path.to_path_buf(),
            log_dir,
        }
    }

    /// Pipelines to run for `hook`: user hooks first, then approved project hooks.
    fn pipelines(&self, hook: &str) -> Result<Vec<(Source, Pipeline)>> {
        if self.opts.no_hooks {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for (source, table) in [(Source::User, &self.user), (Source::Project, &self.project)] {
            let Some(pipeline) = table.get(hook).and_then(parse_pipeline) else {
                continue;
            };
            if source == Source::User || self.approve(hook, &pipeline)? {
                out.push((source, pipeline));
            }
        }
        Ok(out)
    }

    /// Run blocking hooks (`pre-*`) to completion; any failure aborts.
    pub fn run(&self, hook_types: &[&str], cwd: &Path, vars: &Vars) -> Result<()> {
        for hook in hook_types {
            for (source, pipeline) in self.pipelines(hook)? {
                for step in &pipeline {
                    let mut children: Vec<(String, Child)> = Vec::new();
                    for (name, template) in step {
                        let label = match name {
                            Some(n) => format!("{hook} {}:{n}", source.label()),
                            None => format!("{hook} {}", source.label()),
                        };
                        let cmd = template::render(template, &hook_vars(vars, hook, name, cwd))
                            .with_context(|| format!("{label}: bad template"))?;
                        ui::progress(&format!("Running {label}"));
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

    /// Start background hooks (`post-*`), detached, logging to `.git/pm-logs/`.
    pub fn spawn(&self, hook: &str, cwd: &Path, vars: &Vars) -> Result<()> {
        for (source, pipeline) in self.pipelines(hook)? {
            // One shell runs the whole pipeline so steps stay ordered while detached.
            let mut script = String::from("set -e\npids=\n");
            let mut names = Vec::new();
            for step in &pipeline {
                for (name, template) in step {
                    let cmd = template::render(template, &hook_vars(vars, hook, name, cwd))?;
                    script.push_str(&format!("( {cmd}\n) &\npids=\"$pids $!\"\n"));
                    names.push(name.clone().unwrap_or_else(|| "cmd".into()));
                }
                script.push_str("for pid in $pids; do wait \"$pid\"; done\npids=\n");
            }
            fs::create_dir_all(&self.log_dir)?;
            let branch = vars
                .get("branch")
                .map(|b| template::sanitize(b))
                .unwrap_or_default();
            let log_path = self
                .log_dir
                .join(format!("{branch}-{}-{hook}.log", source.label()));
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
            ui::info(&format!(
                "Started {} {hook} ({}) in background; log @ {}",
                source.label(),
                names.join(", "),
                ui::tildify(&log_path)
            ));
        }
        Ok(())
    }

    /// Project hooks come from the repository, so (like worktrunk) they need
    /// approval before they run. Approvals live in the repo's local git config,
    /// which a clone can't pre-populate.
    fn approve(&self, hook: &str, pipeline: &Pipeline) -> Result<bool> {
        let commands: Vec<&str> = pipeline.iter().flatten().map(|(_, c)| c.as_str()).collect();
        let key = format!(
            "{hook}:{:016x}",
            template::fnv1a(commands.join("\n").as_bytes())
        );
        let approved = git::output(&self.repo_path, &["config", "--get-all", APPROVED_HOOK_KEY])
            .unwrap_or_default();
        if approved.lines().any(|l| l == key) {
            return Ok(true);
        }
        if !self.opts.yes {
            if !ui::can_prompt_interactively() {
                ui::warn(&format!(
                    "Skipping unapproved project {hook} hook from .config/wt.toml (rerun with --yes)"
                ));
                return Ok(false);
            }
            ui::warn(&format!(
                ".config/wt.toml wants to run {} hook:",
                hook.bold()
            ));
            for cmd in &commands {
                eprintln!("  {}", cmd.dimmed());
            }
            if !ui::confirm_yes_no("Allow and remember?", false)? {
                return Ok(false);
            }
        }
        git::output(
            &self.repo_path,
            &["config", "--local", "--add", APPROVED_HOOK_KEY, &key],
        )?;
        Ok(true)
    }
}

fn hook_vars(vars: &Vars, hook: &str, name: &Option<String>, cwd: &Path) -> Vars {
    let mut v = vars.clone();
    v.insert("hook_type".into(), hook.to_string());
    v.insert("cwd".into(), cwd.display().to_string());
    if let Some(n) = name {
        v.insert("hook_name".into(), n.clone());
    }
    v
}

fn parse_pipeline(value: &toml::Value) -> Option<Pipeline> {
    fn step(value: &toml::Value) -> Option<Vec<(Option<String>, String)>> {
        match value {
            toml::Value::String(cmd) => Some(vec![(None, cmd.clone())]),
            toml::Value::Table(t) => Some(
                t.iter()
                    .filter_map(|(k, v)| v.as_str().map(|c| (Some(k.clone()), c.to_string())))
                    .collect(),
            ),
            _ => None,
        }
    }
    match value {
        toml::Value::Array(steps) => Some(steps.iter().filter_map(step).collect()),
        other => step(other).map(|s| vec![s]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
