use crate::storage_lock::acquire_storage_lock;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

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

pub(crate) fn write_jsonl_unlocked<S: Serialize>(
    path: impl AsRef<Path>,
    entries: impl AsRef<[S]>,
) -> Result<()> {
    let path = path.as_ref();
    ensure_parent_dir(path)?;
    let temp_path = temp_path(path)?;
    {
        let mut file = File::create(&temp_path)
            .with_context(|| format!("failed to open {}", temp_path.display()))?;
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

pub(crate) fn append_jsonl_unlocked(
    path: impl AsRef<Path>,
    value: &impl Serialize,
    label: impl AsRef<str>,
) -> Result<()> {
    let path = path.as_ref();
    let mut bytes = serde_json::to_vec(value).with_context(|| {
        format!(
            "failed to serialize {} for {}",
            label.as_ref(),
            path.display()
        )
    })?;
    bytes.push(b'\n');

    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))?;
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

fn ensure_parent_dir(path: impl AsRef<Path>) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}
