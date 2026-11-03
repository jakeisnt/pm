//! The one place `p` shells out to git.

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd);
    command
}

/// Run git and return trimmed stdout; on failure the error carries git's stderr.
pub fn output(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = command(cwd, args).output().context("failed to run git")?;
    if !output.status.success() {
        // Some failures (e.g. "nothing to commit") only explain themselves on stdout.
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let why = if stderr.trim().is_empty() {
            stdout
        } else {
            stderr
        };
        bail!(
            "git {} failed in {}: {}",
            args.first().copied().unwrap_or_default(),
            cwd.display(),
            why.trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim_end().to_string())
}

/// Run git with the terminal attached, so its own output reaches the user.
pub fn run(cwd: &Path, args: &[&str]) -> Result<()> {
    let status = command(cwd, args).status().context("failed to run git")?;
    if !status.success() {
        bail!(
            "git {} failed in {}",
            args.first().copied().unwrap_or_default(),
            cwd.display()
        );
    }
    Ok(())
}

/// Run git silently and report whether it succeeded.
pub fn succeeds(cwd: &Path, args: &[&str]) -> bool {
    command(cwd, args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Top-level directory of the repository containing `path`.
pub fn repo_root(path: &Path) -> Option<PathBuf> {
    output(path, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
}

pub fn branch_exists(cwd: &Path, branch: &str) -> bool {
    succeeds(
        cwd,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
}

pub fn remote_branch_exists(cwd: &Path, branch: &str) -> bool {
    succeeds(
        cwd,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/origin/{branch}"),
        ],
    )
}

pub fn is_ancestor(cwd: &Path, ancestor: &str, rev: &str) -> bool {
    succeeds(cwd, &["merge-base", "--is-ancestor", ancestor, rev])
}

pub fn is_clean(cwd: &Path) -> Result<bool> {
    Ok(output(cwd, &["status", "--porcelain"])?.trim().is_empty())
}

pub fn ensure_clean(cwd: &Path, label: &str) -> Result<()> {
    if !is_clean(cwd)? {
        bail!("{label} has uncommitted changes; commit or stash them first");
    }
    Ok(())
}

/// `owner/repo` for a repository whose `origin` points at GitHub.
pub fn github_full_name(path: &Path) -> Option<String> {
    parse_github_remote(&output(path, &["remote", "get-url", "origin"]).ok()?)
}

pub fn parse_github_remote(url: &str) -> Option<String> {
    let path = [
        "git@github.com:",
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix))?;
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        None
    } else {
        Some(format!("{owner}/{repo}"))
    }
}

/// Clone `url` into `target`, authenticating over HTTPS with `token` if given.
pub fn clone(url: &str, target: &Path, token: Option<&str>) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut clone = Command::new("git");
    if let Some(token) = token {
        let credentials = BASE64.encode(format!("x-access-token:{token}"));
        clone
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Basic {credentials}"),
            );
    }
    let status = clone
        .arg("clone")
        .arg(url)
        .arg(target)
        .status()
        .context("failed to run git clone")?;
    if !status.success() {
        bail!("git clone failed for {url}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_remotes() {
        for url in [
            "git@github.com:jakeisnt/pm.git",
            "https://github.com/jakeisnt/pm",
            "ssh://git@github.com/jakeisnt/pm.git/",
        ] {
            assert_eq!(parse_github_remote(url).as_deref(), Some("jakeisnt/pm"));
        }
        assert_eq!(parse_github_remote("https://gitlab.com/a/b"), None);
        assert_eq!(parse_github_remote("git@github.com:a/b/c"), None);
    }
}
