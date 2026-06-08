use anyhow::Result;
use directories::BaseDirs;
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

const DEFAULT_DEPTH: usize = 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    search_roots: Option<Vec<String>>,
    search_depth: Option<usize>,
}

pub fn config_path() -> Result<PathBuf> {
    Ok(env::current_dir()?.join("config.json"))
}

fn load_config() -> Config {
    fs::read_to_string(config_path().unwrap())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Config {
            search_roots: None,
            search_depth: None,
        })
}

pub fn roots() -> Vec<PathBuf> {
    load_config()
        .search_roots
        .unwrap_or_else(|| {
            vec![
                BaseDirs::new()
                    .unwrap()
                    .home_dir()
                    .join("Documents")
                    .to_string_lossy()
                    .to_string(),
            ]
        })
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

pub fn search_depth() -> usize {
    load_config().search_depth.unwrap_or(DEFAULT_DEPTH)
}
