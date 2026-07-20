use crate::storage_lock::acquire_storage_lock;
#[cfg(target_os = "linux")]
use crate::storage_lock::{STATE_DIR_MODE, STATE_FILE_MODE};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

pub fn read_jsonl<D>(path: impl AsRef<Path>) -> Result<Vec<D>>
where
    D: for<'a> Deserialize<'a>,
{
    let path = path.as_ref();
    let state_dir = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let _lock = acquire_storage_lock(state_dir)?;
    read_jsonl_unlocked(path)
}

pub(crate) fn read_jsonl_unlocked<D>(path: impl AsRef<Path>) -> Result<Vec<D>>
where
    D: for<'a> Deserialize<'a>,
{
    let path = path.as_ref();

    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to open {}", path.display())),
    };

    let mut lines = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "failed to read line {} from {}",
                line_number + 1,
                path.display()
            )
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let deserialize = serde_json::from_str(&line).with_context(|| {
            format!(
                "failed to parse line {} from {}",
                line_number + 1,
                path.display()
            )
        })?;

        lines.push(deserialize);
    }

    Ok(lines)
}

fn ensure_parent_dir(path: impl AsRef<Path>) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(target_os = "linux")]
        builder.mode(STATE_DIR_MODE);
        builder
            .create(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

fn temp_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut file_name = path
        .as_ref()
        .file_name()
        .map(ToOwned::to_owned)
        .with_context(|| format!("no file name in path {}", path.as_ref().display()))?;
    file_name.push(".tmp");
    Ok(path.as_ref().with_file_name(file_name))
}

pub(crate) fn write_jsonl_unlocked<S: Serialize>(
    path: impl AsRef<Path>,
    entries: impl AsRef<[S]>,
) -> Result<()> {
    let path = path.as_ref();
    ensure_parent_dir(path)?;
    let temp_path = temp_path(path)?;
    {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(target_os = "linux")]
        options.mode(STATE_FILE_MODE);
        let mut file = options
            .open(&temp_path)
            .with_context(|| format!("failed to open {}", temp_path.display()))?;
        #[cfg(target_os = "linux")]
        file.set_permissions(fs::Permissions::from_mode(STATE_FILE_MODE))
            .with_context(|| format!("failed to set permissions on {}", temp_path.display()))?;
        for entry in entries.as_ref() {
            serde_json::to_writer(&mut file, entry)
                .with_context(|| format!("failed to write {}", temp_path.display()))?;
            file.write_all(b"\n")
                .with_context(|| format!("failed to write {}", temp_path.display()))?;
        }
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temp_path.display()))?;
    }

    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to replace {} with {}",
            path.display(),
            temp_path.display()
        )
    })?;

    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn mode(path: impl AsRef<Path>) -> Result<u32> {
        Ok(fs::metadata(path)?.permissions().mode() & 0o777)
    }

    #[test]
    fn write_creates_private_directory_and_file() -> Result<()> {
        let temp = TempDir::new()?;
        let state_dir = temp.path().join("state");
        let path = state_dir.join("active.jsonl");

        write_jsonl_unlocked(&path, ["entry"])?;

        assert_eq!(mode(state_dir)?, 0o700);
        assert_eq!(mode(path)?, 0o600);
        Ok(())
    }

    #[test]
    fn rewrite_replaces_file_with_private_permissions() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("active.jsonl");
        let temp_path = temp_path(&path)?;
        fs::write(&temp_path, "stale")?;
        fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o644))?;

        write_jsonl_unlocked(&path, ["entry"])?;

        assert_eq!(mode(path)?, 0o600);
        Ok(())
    }
}
