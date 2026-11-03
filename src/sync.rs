//! Keeping the project index in sync with disk and GitHub, off the hot path.
//!
//! Foreground commands only read the SQLite index. When it is older than the
//! configured `syncInterval`, they spawn a detached `p index --quiet` worker
//! and carry on with what's already indexed. A lock file keeps it to one
//! worker at a time. `p index --watch` runs the worker continuously instead.
//!
//! GitHub is expected to fail often. A failed GitHub pass never undoes the
//! local pass or drops repos indexed earlier, and repeated failures back off
//! (5 min doubling to an hour) instead of hammering it.

use crate::{config, db, github::GitHub, github_auth, ui};
use anyhow::{Context, Result};
use colored::Colorize;
use rusqlite::Connection;
use std::{
    env,
    fs::{File, TryLockError},
    process::{Command, Stdio},
    time::Duration,
};

const LAST_SYNC: &str = "last_sync";
const LAST_SPAWN: &str = "last_sync_spawn";
const REMOTE_ATTEMPT: &str = "remote_sync_attempt";
const REMOTE_FAILURES: &str = "remote_sync_failures";
/// Set to skip the automatic background sync (tests, scripts).
const DISABLE_ENV: &str = "PM_NO_BACKGROUND_SYNC";

pub struct Synced {
    pub local: usize,
    pub remote: Remote,
}

/// How the GitHub half of a sync went. It never fails the sync: the local
/// index and previously indexed remote repos are kept either way.
pub enum Remote {
    Indexed(usize),
    NotLoggedIn,
    /// Skipped because recent attempts failed; retried after the backoff.
    BackingOff {
        retry_in: Duration,
    },
    Failed(String),
}

/// Sync local roots and (when authenticated) GitHub into the index.
/// Returns `None` when another sync already holds the lock. `force` is an
/// explicit `p index`: it ignores the GitHub backoff and retries briefly,
/// since someone is waiting; background syncs are patient instead.
pub async fn run(db: &Connection, force: bool) -> Result<Option<Synced>> {
    let lock = File::create(db::data_dir()?.join("sync.lock"))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(e).context("failed to lock sync"),
    }

    let local = db::scan(db)?;
    // Record the local pass before touching the network, so a GitHub outage
    // doesn't leave the index looking unsynced (and foreground rescanning).
    db::set_meta(db, LAST_SYNC, &db::now().to_string())?;
    let remote = sync_remote(db, force).await?;
    Ok(Some(Synced { local, remote }))
}

async fn sync_remote(db: &Connection, force: bool) -> Result<Remote> {
    let Some(token) = github_auth::load_token()? else {
        return Ok(Remote::NotLoggedIn);
    };
    let failures = meta_time(db, REMOTE_FAILURES);
    let retry_at = meta_time(db, REMOTE_ATTEMPT) + remote_backoff(failures).as_millis() as i64;
    if !force && failures > 0 && db::now() < retry_at {
        return Ok(Remote::BackingOff {
            retry_in: Duration::from_millis((retry_at - db::now()) as u64),
        });
    }
    db::set_meta(db, REMOTE_ATTEMPT, &db::now().to_string())?;
    // Someone is waiting on an explicit `p index`; the detached worker can be patient.
    let github = GitHub::new(Some(token));
    let github = if force { github } else { github.patient() };
    match github.user_repos().await {
        Ok(repos) => {
            db::transaction(db, || {
                for repo in &repos {
                    db::upsert_remote_project(db, &repo.full_name)?;
                }
                Ok(())
            })?;
            db::set_meta(db, REMOTE_FAILURES, "0")?;
            Ok(Remote::Indexed(repos.len()))
        }
        Err(e) => {
            db::set_meta(db, REMOTE_FAILURES, &(failures + 1).to_string())?;
            Ok(Remote::Failed(format!("{e:#}")))
        }
    }
}

/// 0 failures: none; then 5 min, 10, 20, 40, capped at an hour.
fn remote_backoff(failures: i64) -> Duration {
    if failures <= 0 {
        return Duration::ZERO;
    }
    let minutes = 5u64.saturating_mul(1 << (failures - 1).min(4));
    Duration::from_secs(minutes.min(60) * 60)
}

/// Sync every `interval`, forever: a long-running background worker.
pub async fn watch(db: &Connection, interval: Duration) -> Result<()> {
    ui::info(&format!(
        "syncing every {}s (ctrl-c to stop)",
        interval.as_secs()
    ));
    loop {
        match run(db, false).await {
            Ok(Some(synced)) => report(&synced),
            Ok(None) => ui::info("another sync is running; skipping"),
            Err(e) => ui::warn(&format!("sync failed: {e:#}")),
        }
        tokio::time::sleep(interval).await;
    }
}

pub fn report(synced: &Synced) {
    let local = format!("indexed {} local", synced.local);
    match &synced.remote {
        Remote::Indexed(n) => ui::success(&format!("{local}, {n} remote")),
        Remote::NotLoggedIn => ui::success(&format!(
            "{local}; GitHub skipped (run {})",
            "p github login".bold()
        )),
        Remote::BackingOff { retry_in } => ui::success(&format!(
            "{local}; GitHub skipped after recent failures (retrying in {}m; {} forces it)",
            retry_in.as_secs().div_ceil(60),
            "p index".bold()
        )),
        Remote::Failed(why) => {
            ui::success(&local);
            ui::warn(&format!(
                "GitHub sync failed; keeping previously indexed remote repos: {why}"
            ));
        }
    }
}

fn meta_time(db: &Connection, key: &str) -> i64 {
    db::get_meta(db, key)
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Called by foreground commands: never blocks on the network. Starts a
/// background worker when the index is stale; on the very first run, scans
/// local roots inline so there's something to show.
pub fn refresh_in_background(db: &Connection) -> Result<()> {
    if env::var_os(DISABLE_ENV).is_some() {
        return Ok(());
    }
    let last_sync = meta_time(db, LAST_SYNC);
    if last_sync == 0 {
        db::scan(db)?;
    }
    let interval = config::load().sync_interval().as_millis() as i64;
    let last_attempt = last_sync.max(meta_time(db, LAST_SPAWN));
    if db::now() - last_attempt < interval {
        return Ok(());
    }
    db::set_meta(db, LAST_SPAWN, &db::now().to_string())?;
    spawn_worker()
}

fn spawn_worker() -> Result<()> {
    let mut command = Command::new(env::current_exe()?);
    command
        .args(["index", "--quiet", "--background"])
        .env(DISABLE_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group: survives the foreground exiting or ctrl-c.
        command.process_group(0);
    }
    command.spawn().context("failed to start background sync")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_backoff_doubles_and_caps() {
        let mins = |n| remote_backoff(n).as_secs() / 60;
        assert_eq!(mins(0), 0);
        assert_eq!(mins(1), 5);
        assert_eq!(mins(2), 10);
        assert_eq!(mins(4), 40);
        assert_eq!(mins(5), 60);
        assert_eq!(mins(50), 60);
    }
}
