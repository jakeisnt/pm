use crate::{config, project::Project};
use anyhow::{Context, Result};
use directories::BaseDirs;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{fs, path::Path};
use uuid::Uuid;
use walkdir::WalkDir;

pub async fn connect() -> Result<SqlitePool> {
    let dir = BaseDirs::new()
        .context("home dir unavailable")?
        .data_dir()
        .join("pm");
    fs::create_dir_all(&dir)?;
    let options = SqliteConnectOptions::new()
        .filename(dir.join("pm.db"))
        .create_if_missing(true);
    Ok(SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
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

pub async fn begin_immediate_transaction(db: &SqlitePool) -> Result<()> {
    sqlx::query("BEGIN IMMEDIATE").execute(db).await?;
    Ok(())
}

pub async fn commit_transaction(db: &SqlitePool) -> Result<()> {
    sqlx::query("COMMIT").execute(db).await?;
    Ok(())
}

pub async fn rollback_transaction(db: &SqlitePool) -> Result<()> {
    sqlx::query("ROLLBACK").execute(db).await?;
    Ok(())
}

pub async fn upsert_project(db: &SqlitePool, path: &Path) -> Result<()> {
    let path = crate::git::repo_root(path).unwrap_or_else(|| path.to_path_buf());
    let name = path.file_name().unwrap().to_string_lossy();
    let remote = crate::git::github_full_name(&path);
    let owner = github_owner(remote.as_deref());
    let path = path.to_string_lossy().to_string();
    let t = now();

    if let Some(full_name) = remote.as_deref() {
        let remote_path = format!("github://{full_name}");
        sqlx::query("UPDATE projects SET path=?1,name=?2,last_scanned=?3,last_modified=?3,is_git_repo=1,source='local',github_full_name=?4,org_name=?5,updated_at=?3,deleted_at=NULL WHERE path=?6 AND NOT EXISTS (SELECT 1 FROM projects WHERE path=?1)")
            .bind(&path)
            .bind(name.as_ref())
            .bind(t)
            .bind(full_name)
            .bind(owner.as_deref().unwrap_or("_local"))
            .bind(remote_path)
            .execute(db)
            .await?;
    }

    sqlx::query("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) VALUES(?1,?2,?3,?4,?4,1,?4,?4,'local',?5,'personal',COALESCE(?6,'_local')) ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_scanned=excluded.last_scanned,last_modified=excluded.last_modified,is_git_repo=1,source='local',github_full_name=COALESCE(excluded.github_full_name,projects.github_full_name),org_name=COALESCE(?6,projects.org_name),updated_at=excluded.updated_at,deleted_at=NULL")
        .bind(Uuid::new_v4().to_string())
        .bind(&path)
        .bind(name.as_ref())
        .bind(t)
        .bind(remote.as_deref())
        .bind(owner)
        .execute(db)
        .await?;

    if let Some(full_name) = remote.as_deref() {
        sqlx::query("UPDATE projects SET deleted_at=?1,updated_at=?1 WHERE github_full_name=?2 AND source='remote' AND path<>?3")
            .bind(t)
            .bind(full_name)
            .bind(&path)
            .execute(db)
            .await?;
    }

    Ok(())
}

fn github_owner(full_name: Option<&str>) -> Option<String> {
    full_name.and_then(|full_name| {
        full_name
            .split_once('/')
            .map(|(owner, _)| owner.to_string())
    })
}

pub async fn upsert_remote_project(db: &SqlitePool, full_name: &str) -> Result<()> {
    let Some((owner, repo)) = full_name.split_once('/') else {
        anyhow::bail!("invalid GitHub repository name: {full_name}");
    };
    let t = now();
    sqlx::query("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) SELECT ?1,?2,?3,?4,?4,1,?4,?4,'remote',?5,'personal',?6 WHERE NOT EXISTS (SELECT 1 FROM projects WHERE github_full_name=?5 AND source='local') ON CONFLICT(path) DO UPDATE SET name=excluded.name,last_scanned=excluded.last_scanned,last_modified=excluded.last_modified,source='remote',github_full_name=excluded.github_full_name,org_name=excluded.org_name,updated_at=excluded.updated_at,deleted_at=NULL")
        .bind(Uuid::new_v4().to_string())
        .bind(format!("github://{full_name}"))
        .bind(repo)
        .bind(t)
        .bind(full_name)
        .bind(owner)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn mark_project_remote_only(db: &SqlitePool, id: &str, full_name: &str) -> Result<()> {
    let Some((owner, repo)) = full_name.split_once('/') else {
        anyhow::bail!("invalid GitHub repository name: {full_name}");
    };
    let t = now();
    let remote_path = format!("github://{full_name}");

    if let Some(existing_remote) = sqlx::query("SELECT id FROM projects WHERE path=?1 AND id<>?2")
        .bind(&remote_path)
        .bind(id)
        .fetch_optional(db)
        .await?
    {
        let remote_id: String = existing_remote.get("id");
        sqlx::query("UPDATE projects SET name=?1,last_scanned=?2,last_modified=?2,source='remote',github_full_name=?3,org_name=?4,updated_at=?2,deleted_at=NULL WHERE id=?5")
            .bind(repo)
            .bind(t)
            .bind(full_name)
            .bind(owner)
            .bind(remote_id)
            .execute(db)
            .await?;
        sqlx::query("UPDATE projects SET deleted_at=?1,updated_at=?1 WHERE id=?2")
            .bind(t)
            .bind(id)
            .execute(db)
            .await?;
    } else {
        sqlx::query("UPDATE projects SET path=?1,name=?2,last_scanned=?3,last_modified=?3,source='remote',github_full_name=?4,org_name=?5,updated_at=?3,deleted_at=NULL WHERE id=?6")
            .bind(remote_path)
            .bind(repo)
            .bind(t)
            .bind(full_name)
            .bind(owner)
            .bind(id)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub async fn mark_project_deleted(db: &SqlitePool, id: &str) -> Result<()> {
    let t = now();
    sqlx::query("UPDATE projects SET deleted_at=?1,updated_at=?1 WHERE id=?2")
        .bind(t)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn rename_project(
    db: &SqlitePool,
    id: &str,
    name: &str,
    path: &Path,
    github_full_name: Option<&str>,
) -> Result<()> {
    let t = now();
    let result = sqlx::query("UPDATE projects SET name=?1,path=?2,github_full_name=?3,last_modified=?4,updated_at=?4 WHERE id=?5 AND deleted_at IS NULL")
        .bind(name)
        .bind(path.to_string_lossy().as_ref())
        .bind(github_full_name)
        .bind(t)
        .bind(id)
        .execute(db)
        .await?;
    if result.rows_affected() == 0 {
        anyhow::bail!("project no longer exists in the index");
    }
    Ok(())
}

pub async fn scan(db: &SqlitePool) -> Result<usize> {
    let mut indexed = 0;
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
                indexed += 1;
            }
        }
    }
    Ok(indexed)
}

pub async fn projects(db: &SqlitePool) -> Result<Vec<Project>> {
    scan(db).await?;
    let rows = sqlx::query("SELECT id,name,path,source,scope,github_full_name FROM projects WHERE deleted_at IS NULL ORDER BY lower(name), CASE source WHEN 'local' THEN 0 ELSE 1 END, path")
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> SqlitePool {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");
        migrate(&db).await.expect("migrate test database");
        db
    }

    #[tokio::test]
    async fn mark_project_remote_only_reuses_existing_remote_row() {
        let db = test_db().await;
        let t = now();
        sqlx::query("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name,deleted_at) VALUES('local','/tmp/oya','oya',?1,?1,1,?1,?1,'local','ulnd/oya','personal','ulnd',NULL)")
            .bind(t)
            .execute(&db)
            .await
            .expect("insert local project");
        sqlx::query("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name,deleted_at) VALUES('remote','github://ulnd/oya','oya',?1,?1,1,?1,?1,'remote','ulnd/oya','personal','ulnd',?1)")
            .bind(t)
            .execute(&db)
            .await
            .expect("insert deleted remote project");

        mark_project_remote_only(&db, "local", "ulnd/oya")
            .await
            .expect("mark local project remote-only without path conflict");

        let rows = sqlx::query("SELECT id,path,source,deleted_at FROM projects ORDER BY id")
            .fetch_all(&db)
            .await
            .expect("fetch projects");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].get::<Option<i64>, _>("deleted_at").is_some());
        assert_eq!(rows[1].get::<String, _>("path"), "github://ulnd/oya");
        assert_eq!(rows[1].get::<String, _>("source"), "remote");
        assert!(rows[1].get::<Option<i64>, _>("deleted_at").is_none());
    }

    #[tokio::test]
    async fn rename_project_preserves_identity_and_history() {
        let db = test_db().await;
        let t = now();
        sqlx::query("INSERT INTO projects(id,path,name,last_scanned,last_modified,is_git_repo,created_at,updated_at,source,github_full_name,scope,org_name) VALUES('project','/tmp/old','old',?1,?1,1,?1,?1,'local','ulnd/old','personal','ulnd')")
            .bind(t)
            .execute(&db)
            .await
            .expect("insert project");
        sqlx::query("INSERT INTO project_history(id,project_id,opened_at,created_at,updated_at) VALUES('history','project',?1,?1,?1)")
            .bind(t)
            .execute(&db)
            .await
            .expect("insert history");

        rename_project(
            &db,
            "project",
            "new",
            Path::new("/tmp/new"),
            Some("ulnd/new"),
        )
        .await
        .expect("rename indexed project");

        let row =
            sqlx::query("SELECT id,name,path,github_full_name FROM projects WHERE id='project'")
                .fetch_one(&db)
                .await
                .expect("fetch renamed project");
        assert_eq!(row.get::<String, _>("id"), "project");
        assert_eq!(row.get::<String, _>("name"), "new");
        assert_eq!(row.get::<String, _>("path"), "/tmp/new");
        assert_eq!(row.get::<String, _>("github_full_name"), "ulnd/new");
        let history_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM project_history WHERE project_id='project'")
                .fetch_one(&db)
                .await
                .expect("fetch history");
        assert_eq!(history_count, 1);
    }
}
