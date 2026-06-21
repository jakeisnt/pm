use crate::{config, project::Project};
use anyhow::{Context, Result};
use directories::BaseDirs;
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;
use walkdir::WalkDir;

pub async fn connect() -> Result<SqlitePool> {
    let dir = BaseDirs::new()
        .context("home dir unavailable")?
        .data_dir()
        .join("pm");
    fs::create_dir_all(&dir)?;
    let url = format!("sqlite://{}", dir.join("pm.db").display());
    Ok(SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await?)
}

pub async fn migrate(db: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY,path TEXT UNIQUE NOT NULL,name TEXT NOT NULL,last_scanned INTEGER NOT NULL,last_modified INTEGER NOT NULL,is_git_repo INTEGER NOT NULL DEFAULT 1,file_count INTEGER,size_bytes INTEGER,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER,source TEXT DEFAULT 'local',github_full_name TEXT,scope TEXT NOT NULL DEFAULT 'personal',org_name TEXT NOT NULL DEFAULT '_local');CREATE TABLE IF NOT EXISTS project_history(id TEXT PRIMARY KEY,project_id TEXT NOT NULL,opened_at INTEGER NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER);CREATE TABLE IF NOT EXISTS orgs(name TEXT PRIMARY KEY,hidden INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER);INSERT OR IGNORE INTO orgs(name,hidden,created_at,updated_at) VALUES('_local',0,unixepoch()*1000,unixepoch()*1000);")
        .execute(db)
        .await?;
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

pub async fn upsert_project(db: &SqlitePool, path: &Path) -> Result<()> {
    let path = git_repo_root(path)?.unwrap_or_else(|| path.to_path_buf());
    let name = path.file_name().unwrap().to_string_lossy();
    let remote = github_full_name(&path);
    let owner = remote.as_deref().and_then(|full_name| {
        full_name
            .split_once('/')
            .map(|(owner, _)| owner.to_string())
    });
    let t = now();
    sqlx::query("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) VALUES(?1,?2,?3,?4,?4,1,?4,?4,'local',?5,'personal',COALESCE(?6,'_local')) ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_scanned=excluded.last_scanned,last_modified=excluded.last_modified,source='local',github_full_name=COALESCE(excluded.github_full_name,projects.github_full_name),org_name=COALESCE(?6,projects.org_name),updated_at=excluded.updated_at,deleted_at=NULL")
        .bind(Uuid::new_v4().to_string())
        .bind(path.to_string_lossy().to_string())
        .bind(name.as_ref())
        .bind(t)
        .bind(remote)
        .bind(owner)
        .execute(db)
        .await?;
    Ok(())
}

pub fn git_repo_root(path: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let Ok(output) = output else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }
    let root =
        String::from_utf8(output.stdout).context("git returned a non-UTF-8 repository path")?;
    let root = root.trim();
    if root.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(root)))
    }
}

fn github_full_name(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8(output.stdout).ok()?;
    parse_github_remote(url.trim())
}

fn parse_github_remote(url: &str) -> Option<String> {
    let path = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = url.strip_prefix("https://github.com/") {
        rest
    } else if let Some(rest) = url.strip_prefix("http://github.com/") {
        rest
    } else if let Some(rest) = url.strip_prefix("ssh://git@github.com/") {
        rest
    } else {
        return None;
    };
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        None
    } else {
        Some(format!("{owner}/{repo}"))
    }
}

pub async fn mark_project_remote_only(db: &SqlitePool, id: &str, full_name: &str) -> Result<()> {
    let Some((owner, repo)) = full_name.split_once('/') else {
        anyhow::bail!("invalid GitHub repository name: {full_name}");
    };
    let t = now();
    sqlx::query("UPDATE projects SET path=?1,name=?2,last_scanned=?3,last_modified=?3,source='remote',github_full_name=?4,org_name=?5,updated_at=?3,deleted_at=NULL WHERE id=?6")
        .bind(format!("github://{full_name}"))
        .bind(repo)
        .bind(t)
        .bind(full_name)
        .bind(owner)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn scan(db: &SqlitePool) -> Result<()> {
    for root in config::roots() {
        if !root.exists() {
            continue;
        }
        for e in WalkDir::new(root)
            .max_depth(config::search_depth() + 1)
            .into_iter()
            .filter_map(Result::ok)
        {
            if e.file_type().is_dir() && e.path().join(".git").exists() {
                upsert_project(db, e.path()).await?;
            }
        }
    }
    Ok(())
}

pub async fn projects(db: &SqlitePool) -> Result<Vec<Project>> {
    scan(db).await?;
    let rows = sqlx::query("SELECT id,name,path,source,scope,github_full_name FROM projects WHERE deleted_at IS NULL ORDER BY lower(name)")
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| Project {
            id: r.get("id"),
            name: r.get("name"),
            path: r.get("path"),
            source: r.get("source"),
            scope: r.get("scope"),
            github_full_name: r.get("github_full_name"),
        })
        .collect())
}

pub async fn find_project(db: &SqlitePool, name: &str) -> Result<Option<Project>> {
    let ps = projects(db).await?;
    let low = name.to_lowercase();
    Ok(ps
        .iter()
        .find(|p| {
            p.name.to_lowercase() == low
                || p.github_full_name.as_deref().unwrap_or("").to_lowercase() == low
        })
        .cloned()
        .or_else(|| {
            ps.into_iter()
                .find(|p| p.name.to_lowercase().contains(&low))
        }))
}

pub async fn touch(db: &SqlitePool, p: &Project) -> Result<()> {
    let t = now();
    sqlx::query("INSERT INTO project_history(id,project_id,opened_at,created_at,updated_at) VALUES(?1,?2,?3,?3,?3)")
        .bind(Uuid::new_v4().to_string())
        .bind(&p.id)
        .bind(t)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn orgs(db: &SqlitePool) -> Result<Vec<(String, i64)>> {
    let rows = sqlx::query("SELECT name,hidden FROM orgs WHERE deleted_at IS NULL ORDER BY name")
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(|r| (r.get(0), r.get(1))).collect())
}

pub async fn set_org_hidden(db: &SqlitePool, name: String, hidden: bool) -> Result<()> {
    let t = now();
    sqlx::query("INSERT INTO orgs(name,hidden,created_at,updated_at) VALUES(?1,?2,?3,?3) ON CONFLICT(name) DO UPDATE SET hidden=excluded.hidden,updated_at=excluded.updated_at,deleted_at=NULL")
        .bind(name)
        .bind(hidden as i64)
        .bind(t)
        .execute(db)
        .await?;
    Ok(())
}
