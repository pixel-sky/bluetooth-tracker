use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

const LOCK_FILE_NAME: &str = ".keychron-tracker.lock";

pub(crate) struct StorageLock {
    _file: File,
}

pub(crate) fn acquire_storage_lock(state_dir: impl AsRef<Path>) -> Result<StorageLock> {
    let lock_path = state_dir.as_ref().join(LOCK_FILE_NAME);
    Ok(StorageLock {
        _file: open_and_lock(&lock_path)?,
    })
}

fn open_and_lock(lock_path: impl AsRef<Path>) -> Result<File> {
    let lock_path = lock_path.as_ref();
    ensure_parent_dir(lock_path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .with_context(|| format!("failed to open {}", lock_path.display()))?;
    lock_file(&file).with_context(|| format!("failed to lock {}", lock_path.display()))?;
    Ok(file)
}

fn ensure_parent_dir(path: impl AsRef<Path>) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn lock_file(file: &File) -> std::io::Result<()> {
    use std::{os::fd::AsRawFd, os::raw::c_int};

    const LOCK_EX: c_int = 2;

    unsafe extern "C" {
        fn flock(fd: c_int, operation: c_int) -> c_int;
    }

    let result = unsafe { flock(file.as_raw_fd(), LOCK_EX) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn lock_file(_file: &File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn storage_lock_is_created_in_the_storage_directory() -> Result<()> {
        let temp = TempDir::new()?;
        let _lock = acquire_storage_lock(temp.path())?;

        assert!(temp.path().join(LOCK_FILE_NAME).is_file());
        Ok(())
    }
}
