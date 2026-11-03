//! Projects and how a name typed on the command line becomes a local checkout.

use crate::{db, git, github::GitHub, github_auth, ui};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use serde::Serialize;
use sqlx::SqlitePool;
use std::{
    fmt,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub source: String,
    pub scope: Option<String>,
    pub github_full_name: Option<String>,
}

impl fmt::Display for Project {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<28} {}", self.name, self.path)
    }
}

/// Find `name` in the index (or as `owner/repo` on GitHub) and make sure it is
/// checked out locally, cloning after confirmation if needed.
pub async fn resolve(db: &SqlitePool, name: &str) -> Result<Option<Project>> {
    match db::find_project(db, name).await? {
        Some(project) => ensure_local(db, project).await.map(Some),
        None => clone_from_github(db, name).await,
    }
}

/// Like [`resolve`], but fall back to the fuzzy picker (prefilled with `name`)
/// when nothing matches. With no name, go straight to the picker.
pub async fn resolve_or_pick(db: &SqlitePool, name: Option<&str>) -> Result<Option<Project>> {
    if let Some(name) = name {
        if let Some(project) = resolve(db, name).await? {
            return Ok(Some(project));
        }
        ui::warn(&format!("no project found for {}", name.bold()));
    }
    match pick(db::projects(db).await?, name)? {
        Some(project) => ensure_local(db, project).await.map(Some),
        None => Ok(None),
    }
}

/// Fuzzy-pick a project.
pub fn pick(projects: Vec<Project>, query: Option<&str>) -> Result<Option<Project>> {
    if projects.is_empty() {
        bail!("no projects found")
    }
    let input = projects
        .iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}",
                ui::style_name(&p.name),
                ui::style_path(&p.path),
                p.id
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let Some(line) = ui::fuzzy_select("Select project>", &input, query)? else {
        return Ok(None);
    };
    let id = line.rsplit('\t').next().unwrap_or_default();
    projects
        .into_iter()
        .find(|p| p.id == id)
        .context("selected project no longer exists")
        .map(Some)
}

async fn ensure_local(db: &SqlitePool, project: Project) -> Result<Project> {
    if let Some(name) = project.path.strip_prefix("github://") {
        return clone_from_github(db, name)
            .await?
            .with_context(|| format!("{} is not available locally", project.path));
    }
    if Path::new(&project.path).exists() {
        return Ok(project);
    }
    let full_name = project
        .github_full_name
        .clone()
        .or_else(|| infer_github_full_name(&project.path));
    let Some(full_name) = full_name else {
        return Ok(project);
    };
    db::mark_project_remote_only(db, &project.id, &full_name).await?;
    clone_from_github(db, &full_name)
        .await?
        .with_context(|| format!("{full_name} is not available locally"))
}

/// `~/Documents/<owner>/<repo>` → `owner/repo`.
fn infer_github_full_name(path: &str) -> Option<String> {
    let path = Path::new(path);
    let repo = path.file_name()?.to_str()?;
    let owner = path.parent()?.file_name()?.to_str()?;
    Some(format!("{owner}/{repo}"))
}

/// Where `p` keeps GitHub checkouts.
pub fn github_checkout_path(owner: &str, repo: &str) -> Result<PathBuf> {
    Ok(directories::BaseDirs::new()
        .context("home dir unavailable")?
        .home_dir()
        .join("Documents")
        .join(owner)
        .join(repo))
}

/// Clone `owner/repo` into the standard path (after asking) and index it.
/// Returns `None` for names that aren't `owner/repo` or when the user declines.
async fn clone_from_github(db: &SqlitePool, name: &str) -> Result<Option<Project>> {
    let Some((owner, repo)) = name.split_once('/') else {
        return Ok(None);
    };
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Ok(None);
    }

    let github = GitHub::new(github_auth::token_or_login()?);
    let info = github.repo(name).await.with_context(|| {
        if github.token().is_some() {
            "check that your token has private-repository access and org SSO authorization"
        } else {
            "authenticate with `gh auth login` or `p github login` to access private repositories"
        }
    })?;
    let target = github_checkout_path(owner, repo)?;

    if !target.exists() {
        if !ui::can_prompt_interactively() {
            ui::warn(&format!(
                "{} is not available locally in {}; skipping interactive clone prompt",
                info.full_name.bold(),
                ui::terminal_interface().dimmed(),
            ));
            return Ok(None);
        }
        let prompt = format!(
            "{} is not available locally. Clone it to {}?",
            info.full_name,
            target.display()
        );
        if !ui::confirm_yes_no(&prompt, true)? {
            return Ok(None);
        }
        git::clone(&info.clone_url, &target, github.token())?;
    }

    db::upsert_project(db, &target).await?;
    db::find_project(db, repo)
        .await?
        .with_context(|| format!("cloned {}, but could not index it", info.full_name))
        .map(Some)
}
