use anyhow::{Context, Result};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

#[cfg(target_os = "linux")]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

const LOCK_FILE_NAME: &str = ".bluetooth-tracker.lock";
#[cfg(target_os = "linux")]
pub(crate) const STATE_DIR_MODE: u32 = 0o700;
#[cfg(target_os = "linux")]
pub(crate) const STATE_FILE_MODE: u32 = 0o600;

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
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(target_os = "linux")]
    options.mode(STATE_FILE_MODE);
    let file = options
        .open(lock_path)
        .with_context(|| format!("failed to open {}", lock_path.display()))?;
    lock_file(&file).with_context(|| format!("failed to lock {}", lock_path.display()))?;
    Ok(file)
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
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn storage_lock_is_created_in_the_storage_directory() -> Result<()> {
        let temp = TempDir::new()?;
        let state_dir = temp.path().join("state");
        let _lock = acquire_storage_lock(&state_dir)?;
        let lock_path = state_dir.join(".bluetooth-tracker.lock");

        assert!(lock_path.is_file());
        #[cfg(target_os = "linux")]
        {
            assert_eq!(fs::metadata(state_dir)?.permissions().mode() & 0o777, 0o700);
            assert_eq!(fs::metadata(lock_path)?.permissions().mode() & 0o777, 0o600);
        }
        Ok(())
    }
}
