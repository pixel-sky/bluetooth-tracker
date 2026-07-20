use anyhow::{Result, anyhow};
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerPaths {
    state_dir: PathBuf,
}

impl TrackerPaths {
    pub fn new(state_dir: impl AsRef<Path>) -> Self {
        Self {
            state_dir: state_dir.as_ref().to_owned(),
        }
    }

    pub fn from_default_state_dir() -> Result<Self> {
        Ok(Self::new(default_state_dir()?))
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn spans_path(&self) -> PathBuf {
        self.state_dir.join("spans.jsonl")
    }

    pub fn actives_path(&self) -> PathBuf {
        self.state_dir.join("active.jsonl")
    }
}

fn default_state_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("bluetooth-tracker"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".local/state/bluetooth-tracker"))
}

pub fn default_user_systemd_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("systemd/user"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/systemd/user"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_paths_use_fixed_filenames_in_one_directory() {
        let paths = TrackerPaths::new("/tmp/bluetooth state");

        assert_eq!(paths.state_dir(), Path::new("/tmp/bluetooth state"));
        assert_eq!(
            paths.spans_path(),
            Path::new("/tmp/bluetooth state/spans.jsonl")
        );
        assert_eq!(
            paths.actives_path(),
            Path::new("/tmp/bluetooth state/active.jsonl")
        );
        assert_eq!(paths.spans_path().parent(), paths.actives_path().parent());
    }
}
