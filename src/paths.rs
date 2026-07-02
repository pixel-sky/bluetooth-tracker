use anyhow::{anyhow, Result};
use std::{env, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerPaths {
    pub log_path: PathBuf,
    pub state_path: PathBuf,
}

impl TrackerPaths {
    pub fn from_overrides(log_path: Option<PathBuf>, state_path: Option<PathBuf>) -> Result<Self> {
        let state_dir = default_state_dir()?;
        Ok(Self {
            log_path: log_path.unwrap_or_else(|| state_dir.join("spans.jsonl")),
            state_path: state_path.unwrap_or_else(|| state_dir.join("active.json")),
        })
    }
}

fn default_state_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("keychron-tracker"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".local/state/keychron-tracker"))
}

pub fn default_user_systemd_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("systemd/user"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/systemd/user"))
}
