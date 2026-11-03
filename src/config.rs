//! User configuration (`config.json`).
//!
//! Lives in the platform config directory (`~/.config/pm/config.json` on Linux,
//! `~/Library/Application Support/pm/config.json` on macOS). For compatibility,
//! a `config.json` in the current directory is used when that file is absent.

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::Deserialize;
use std::{fs, path::PathBuf, time::Duration};

const DEFAULT_DEPTH: usize = 2;
const DEFAULT_SYNC_INTERVAL_SECS: u64 = 300;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    search_roots: Option<Vec<String>>,
    search_depth: Option<usize>,
    sync_interval: Option<u64>,
}

impl Config {
    /// Directories scanned for git repositories (default: `~/Documents`).
    pub fn roots(&self) -> Vec<PathBuf> {
        match &self.search_roots {
            Some(roots) => roots.iter().map(|r| expand_home(r)).collect(),
            None => home().map(|h| h.join("Documents")).into_iter().collect(),
        }
    }

    /// How many directory levels below each root to search.
    pub fn depth(&self) -> usize {
        self.search_depth.unwrap_or(DEFAULT_DEPTH)
    }

    /// How stale the index may get before a background sync starts.
    pub fn sync_interval(&self) -> Duration {
        Duration::from_secs(self.sync_interval.unwrap_or(DEFAULT_SYNC_INTERVAL_SECS))
    }
}

fn home() -> Option<PathBuf> {
    BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if path == "~" => home().unwrap_or_else(|| PathBuf::from(path)),
        _ => PathBuf::from(path),
    }
}

/// Where `p config set` writes, and the first place `load` looks.
pub fn path() -> Result<PathBuf> {
    Ok(BaseDirs::new()
        .context("home dir unavailable")?
        .config_dir()
        .join("pm")
        .join("config.json"))
}

/// The config file in use: the user config if it exists, else `./config.json`.
pub fn active_path() -> Result<PathBuf> {
    let user = path()?;
    if user.exists() {
        return Ok(user);
    }
    let legacy = std::env::current_dir()?.join("config.json");
    Ok(if legacy.exists() { legacy } else { user })
}

/// Load the config, warning (rather than failing) when it can't be parsed.
pub fn load() -> Config {
    let Ok(path) = active_path() else {
        return Config::default();
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return Config::default();
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        crate::ui::warn(&format!("ignoring invalid {}: {e}", path.display()));
        Config::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numbers_and_expands_home() {
        let config: Config =
            serde_json::from_str(r#"{"searchRoots": ["~/code", "/abs"], "searchDepth": 3}"#)
                .unwrap();
        assert_eq!(config.depth(), 3);
        let roots = config.roots();
        assert!(roots[0].ends_with("code") && roots[0].is_absolute());
        assert_eq!(roots[1], PathBuf::from("/abs"));
    }
}
