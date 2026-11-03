//! SQLite project index.

use crate::{config, git, project::Project};
use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use walkdir::WalkDir;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY,path TEXT UNIQUE NOT NULL,name TEXT NOT NULL,last_scanned INTEGER NOT NULL,last_modified INTEGER NOT NULL,is_git_repo INTEGER NOT NULL DEFAULT 1,file_count INTEGER,size_bytes INTEGER,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER,source TEXT DEFAULT 'local',github_full_name TEXT,scope TEXT NOT NULL DEFAULT 'personal',org_name TEXT NOT NULL DEFAULT '_local');
CREATE TABLE IF NOT EXISTS project_history(id TEXT PRIMARY KEY,project_id TEXT NOT NULL,opened_at INTEGER NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER);
CREATE TABLE IF NOT EXISTS orgs(name TEXT PRIMARY KEY,hidden INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,deleted_at INTEGER);
INSERT OR IGNORE INTO orgs(name,hidden,created_at,updated_at) VALUES('_local',0,unixepoch()*1000,unixepoch()*1000);
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
";

/// `pm`'s directory under the platform data dir (index, sync lock).
pub fn data_dir() -> Result<PathBuf> {
    let dir = BaseDirs::new()
        .context("home dir unavailable")?
        .data_dir()
        .join("pm");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Open (creating if needed) the index. WAL lets the foreground read while a
/// background sync writes; the busy timeout covers the brief write overlaps.
pub fn open() -> Result<Connection> {
    let path = data_dir()?.join("pm.db");
    let db = Connection::open(&path)
        .with_context(|| format!("failed to open project index {}", path.display()))?;
    db.busy_timeout(Duration::from_secs(10))?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    db.execute_batch(SCHEMA)?;
    Ok(db)
}

pub fn get_meta(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}

pub fn set_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Run `f` inside `BEGIN IMMEDIATE`, committing on success and rolling back on error.
pub fn transaction<T>(db: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    db.execute_batch("BEGIN IMMEDIATE")?;
    match f() {
        Ok(value) => {
            db.execute_batch("COMMIT")?;
            Ok(value)
        }
        Err(error) => {
            let _ = db.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn split_full_name(full_name: &str) -> Result<(&str, &str)> {
    full_name
        .split_once('/')
        .with_context(|| format!("invalid GitHub repository name: {full_name}"))
}

/// A local checkout as seen on disk; gathering it runs git, so it happens
/// outside any transaction.
pub struct LocalRepo {
    path: String,
    name: String,
    remote: Option<String>,
}

impl LocalRepo {
    /// `path` must be a repository root (or is resolved to one).
    pub fn inspect(path: &Path) -> Result<Self> {
        let path = git::repo_root(path).unwrap_or_else(|| path.to_path_buf());
        Ok(Self {
            name: path
                .file_name()
                .context("project path has no name")?
                .to_string_lossy()
                .into_owned(),
            remote: git::github_full_name(&path),
            path: path.to_string_lossy().into_owned(),
        })
    }
}

/// Index the repository containing `path`.
pub fn upsert_project(db: &Connection, path: &Path) -> Result<()> {
    write_local(db, &LocalRepo::inspect(path)?)
}

fn write_local(db: &Connection, repo: &LocalRepo) -> Result<()> {
    let LocalRepo { path, name, remote } = repo;
    let owner = remote
        .as_deref()
        .and_then(|r| r.split_once('/'))
        .map(|(owner, _)| owner);
    let t = now();

    if let Some(full_name) = remote.as_deref() {
        // Adopt a remote-only row for this repo now that it is checked out.
        db.execute(
            "UPDATE projects SET path=?1,name=?2,last_scanned=?3,last_modified=?3,is_git_repo=1,source='local',github_full_name=?4,org_name=?5,updated_at=?3,deleted_at=NULL WHERE path=?6 AND NOT EXISTS (SELECT 1 FROM projects WHERE path=?1)",
            params![path, name, t, full_name, owner.unwrap_or("_local"), format!("github://{full_name}")],
        )?;
    }

    db.execute(
        "INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) VALUES(?1,?2,?3,?4,?4,1,?4,?4,'local',?5,'personal',COALESCE(?6,'_local')) ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_scanned=excluded.last_scanned,last_modified=excluded.last_modified,is_git_repo=1,source='local',github_full_name=COALESCE(excluded.github_full_name,projects.github_full_name),org_name=COALESCE(?6,projects.org_name),updated_at=excluded.updated_at,deleted_at=NULL",
        params![Uuid::new_v4().to_string(), path, name, t, remote, owner],
    )?;

    if let Some(full_name) = remote.as_deref() {
        db.execute(
            "UPDATE projects SET deleted_at=?1,updated_at=?1 WHERE github_full_name=?2 AND source='remote' AND path<>?3",
            params![t, full_name, path],
        )?;
    }
    Ok(())
}

pub fn upsert_remote_project(db: &Connection, full_name: &str) -> Result<()> {
    let (owner, repo) = split_full_name(full_name)?;
    db.execute(
        "INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) SELECT ?1,?2,?3,?4,?4,1,?4,?4,'remote',?5,'personal',?6 WHERE NOT EXISTS (SELECT 1 FROM projects WHERE github_full_name=?5 AND source='local') ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_scanned=excluded.last_scanned,last_modified=excluded.last_modified,source='remote',github_full_name=excluded.github_full_name,org_name=excluded.org_name,updated_at=excluded.updated_at,deleted_at=NULL",
        params![Uuid::new_v4().to_string(), format!("github://{full_name}"), repo, now(), full_name, owner],
    )?;
    Ok(())
}

pub fn mark_project_remote_only(db: &Connection, id: &str, full_name: &str) -> Result<()> {
    let (owner, repo) = split_full_name(full_name)?;
    let t = now();
    let remote_path = format!("github://{full_name}");

    let existing: Option<String> = db
        .query_row(
            "SELECT id FROM projects WHERE path=?1 AND id<>?2",
            params![remote_path, id],
            |r| r.get(0),
        )
        .optional()?;
    match existing {
        Some(remote_id) => {
            db.execute(
                "UPDATE projects SET name=?1,last_scanned=?2,last_modified=?2,source='remote',github_full_name=?3,org_name=?4,updated_at=?2,deleted_at=NULL WHERE id=?5",
                params![repo, t, full_name, owner, remote_id],
            )?;
            mark_project_deleted(db, id)?;
        }
        None => {
            db.execute(
                "UPDATE projects SET path=?1,name=?2,last_scanned=?3,last_modified=?3,source='remote',github_full_name=?4,org_name=?5,updated_at=?3,deleted_at=NULL WHERE id=?6",
                params![remote_path, repo, t, full_name, owner, id],
            )?;
        }
    }
    Ok(())
}

pub fn mark_project_deleted(db: &Connection, id: &str) -> Result<()> {
    db.execute(
        "UPDATE projects SET deleted_at=?1,updated_at=?1 WHERE id=?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn rename_project(
    db: &Connection,
    id: &str,
    name: &str,
    path: &Path,
    github_full_name: Option<&str>,
) -> Result<()> {
    let changed = db.execute(
        "UPDATE projects SET name=?1,path=?2,github_full_name=?3,last_modified=?4,updated_at=?4 WHERE id=?5 AND deleted_at IS NULL",
        params![name, path.to_string_lossy(), github_full_name, now(), id],
    )?;
    if changed == 0 {
        bail!("project no longer exists in the index");
    }
    Ok(())
}

/// Index every git repository under the configured search roots.
///
/// Repositories are inspected in parallel (each needs a git call), then
/// written in one short transaction so foreground commands never wait long.
pub fn scan(db: &Connection) -> Result<usize> {
    let config = config::load();
    let paths: Vec<PathBuf> = config
        .roots()
        .into_iter()
        .filter(|root| root.exists())
        .flat_map(|root| {
            WalkDir::new(root)
                .max_depth(config.depth() + 1)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_dir() && e.path().join(".git").exists())
                .map(|e| e.into_path())
        })
        .collect();

    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = paths.len().div_ceil(workers).max(1);
    let repos: Vec<LocalRepo> = std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|p| LocalRepo::inspect(p).ok())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });

    transaction(db, || {
        for repo in &repos {
            write_local(db, repo)?;
        }
        Ok(repos.len())
    })
}

/// All live projects in the index. Never scans: the index is kept fresh by
/// the background sync (see `sync.rs`).
pub fn projects(db: &Connection) -> Result<Vec<Project>> {
    let mut stmt = db.prepare(
        "SELECT id,name,path,source,scope,github_full_name FROM projects WHERE deleted_at IS NULL ORDER BY lower(name), CASE source WHEN 'local' THEN 0 ELSE 1 END, path",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Project {
            id: r.get(0)?,
            name: r.get(1)?,
            path: r.get(2)?,
            source: r.get(3)?,
            scope: r.get(4)?,
            github_full_name: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Exact name or `owner/repo` match first, then the first name containing `name`.
pub fn find_project(db: &Connection, name: &str) -> Result<Option<Project>> {
    let projects = projects(db)?;
    let low = name.to_lowercase();
    let exact = projects.iter().position(|p| {
        p.name.to_lowercase() == low
            || p.github_full_name
                .as_deref()
                .is_some_and(|f| f.to_lowercase() == low)
    });
    let index = exact.or_else(|| {
        projects
            .iter()
            .position(|p| p.name.to_lowercase().contains(&low))
    });
    Ok(index.map(|i| projects[i].clone()))
}

/// Record that `project` was opened.
pub fn touch(db: &Connection, project: &Project) -> Result<()> {
    db.execute(
        "INSERT INTO project_history(id,project_id,opened_at,created_at,updated_at) VALUES(?1,?2,?3,?3,?3)",
        params![Uuid::new_v4().to_string(), project.id, now()],
    )?;
    Ok(())
}

pub fn orgs(db: &Connection) -> Result<Vec<(String, bool)>> {
    let mut stmt =
        db.prepare("SELECT name,hidden FROM orgs WHERE deleted_at IS NULL ORDER BY name")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn set_org_hidden(db: &Connection, name: &str, hidden: bool) -> Result<()> {
    db.execute(
        "INSERT INTO orgs(name,hidden,created_at,updated_at) VALUES(?1,?2,?3,?3) ON CONFLICT(name) DO UPDATE SET hidden=excluded.hidden,updated_at=excluded.updated_at,deleted_at=NULL",
        params![name, hidden, now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let db = Connection::open_in_memory().expect("open in-memory sqlite");
        db.execute_batch(SCHEMA).expect("create schema");
        db
    }

    #[test]
    fn mark_project_remote_only_reuses_existing_remote_row() {
        let db = test_db();
        let t = now();
        db.execute("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name,deleted_at) VALUES('local','/tmp/oya','oya',?1,?1,1,?1,?1,'local','ulnd/oya','personal','ulnd',NULL)", [t])
            .expect("insert local project");
        db.execute("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name,deleted_at) VALUES('remote','github://ulnd/oya','oya',?1,?1,1,?1,?1,'remote','ulnd/oya','personal','ulnd',?1)", [t])
            .expect("insert deleted remote project");

        mark_project_remote_only(&db, "local", "ulnd/oya")
            .expect("mark local project remote-only without path conflict");

        let mut stmt = db
            .prepare("SELECT path,source,deleted_at FROM projects ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String, Option<i64>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].2.is_some());
        assert_eq!(rows[1].0, "github://ulnd/oya");
        assert_eq!(rows[1].1, "remote");
        assert!(rows[1].2.is_none());
    }

    #[test]
    fn rename_project_preserves_identity_and_history() {
        let db = test_db();
        let t = now();
        db.execute("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) VALUES('project','/tmp/old','old',?1,?1,1,?1,?1,'local','ulnd/old','personal','ulnd')", [t])
            .expect("insert project");
        db.execute("INSERT INTO project_history(id,project_id,opened_at,created_at,updated_at) VALUES('history','project',?1,?1,?1)", [t])
            .expect("insert history");

        rename_project(
            &db,
            "project",
            "new",
            Path::new("/tmp/new"),
            Some("ulnd/new"),
        )
        .expect("rename indexed project");

        let row: (String, String, String) = db
            .query_row(
                "SELECT name,path,github_full_name FROM projects WHERE id='project'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("new".into(), "/tmp/new".into(), "ulnd/new".into()));
        let history: i64 = db
            .query_row(
                "SELECT count(*) FROM project_history WHERE project_id='project'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(history, 1);
    }

    #[test]
    fn transaction_rolls_back_on_error() {
        let db = test_db();
        let result: Result<()> = transaction(&db, || {
            set_org_hidden(&db, "acme", true)?;
            bail!("boom")
        });
        assert!(result.is_err());
        assert!(orgs(&db).unwrap().iter().all(|(name, _)| name != "acme"));
    }
}
