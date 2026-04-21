use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

const INDEXER_LOCK_FILENAME: &str = "indexer.lock";
const INDEXER_STATUS_PREFIX: &str = "indexer-status-";
const INDEXER_STATUS_SUFFIX: &str = ".json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockMetadata {
    pid: u32,
    mode: String,
    workspace_root: String,
    acquired_at: String,
}

pub struct IndexerLock {
    path: PathBuf,
    status_path: PathBuf,
}

impl IndexerLock {
    pub fn acquire(data_dir: &Path, workspace_root: &Path, mode: &str) -> Result<Self, String> {
        let path = data_dir.join(INDEXER_LOCK_FILENAME);
        let metadata = LockMetadata {
            pid: std::process::id(),
            mode: mode.to_string(),
            workspace_root: workspace_root.display().to_string(),
            acquired_at: Utc::now().to_rfc3339(),
        };

        match try_create_lock(&path, &metadata) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                if try_recover_stale_lock(data_dir, &path)? {
                    try_create_lock(&path, &metadata).map_err(|retry_err| format_lock_error(&path, retry_err))?;
                } else {
                    return Err(format_lock_error(&path, err));
                }
            }
            Err(err) => return Err(format_lock_error(&path, err)),
        }

        let status_path = write_status_file(data_dir, &metadata)?;
        Ok(Self { path, status_path })
    }
}

impl Drop for IndexerLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&self.status_path);
    }
}

fn try_create_lock(path: &Path, metadata: &LockMetadata) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    write_metadata(&mut file, metadata).inspect_err(|_| {
        let _ = fs::remove_file(path);
    })?;
    Ok(())
}

fn write_status_file(data_dir: &Path, metadata: &LockMetadata) -> Result<PathBuf, String> {
    let path = status_path(data_dir, metadata.pid);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|err| format!("Failed to create indexer status {}: {}", path.display(), err))?;
    write_metadata(&mut file, metadata)
        .map_err(|err| format!("Failed to write indexer status {}: {}", path.display(), err))?;
    Ok(path)
}

fn write_metadata(file: &mut fs::File, metadata: &LockMetadata) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(metadata)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;
    file.write_all(&json)?;
    file.flush()
}

fn try_recover_stale_lock(data_dir: &Path, lock_path: &Path) -> Result<bool, String> {
    let Some(metadata) = read_lock_metadata(lock_path) else {
        return Ok(false);
    };
    if process_is_running(metadata.pid) {
        return Ok(false);
    }

    fs::remove_file(lock_path)
        .map_err(|err| format!("Failed to remove stale indexer lock {}: {}", lock_path.display(), err))?;
    let stale_status_path = status_path(data_dir, metadata.pid);
    let _ = fs::remove_file(stale_status_path);
    Ok(true)
}

fn read_lock_metadata(path: &Path) -> Option<LockMetadata> {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<LockMetadata>(&content).ok())
}

fn status_path(data_dir: &Path, pid: u32) -> PathBuf {
    data_dir.join(format!("{INDEXER_STATUS_PREFIX}{pid}{INDEXER_STATUS_SUFFIX}"))
}

fn format_lock_error(path: &Path, err: io::Error) -> String {
    if err.kind() != io::ErrorKind::AlreadyExists {
        return format!("Failed to acquire indexer lock {}: {}", path.display(), err);
    }

    let details = read_lock_metadata(path)
        .map(|metadata| {
            format!(
                "existing pid {} ({}) has held this data-dir lock since {} for workspace {}",
                metadata.pid,
                metadata.mode,
                metadata.acquired_at,
                metadata.workspace_root
            )
        })
        .unwrap_or_else(|| "an existing indexer lock file is present".to_string());

    format!(
        "Another indexer is already using {}: {}",
        path.display(),
        details
    )
}

#[cfg(target_os = "windows")]
fn process_is_running(pid: u32) -> bool {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == null_mut() {
        return false;
    }

    let mut exit_code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    unsafe {
        CloseHandle(handle);
    }
    ok != 0 && exit_code == STILL_ACTIVE as u32
}

#[cfg(not(target_os = "windows"))]
fn process_is_running(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(test)]
mod tests {
    use super::{IndexerLock, status_path};

    #[test]
    fn prevents_second_lock_for_same_data_dir() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".codeatlas");
        std::fs::create_dir_all(&data_dir).unwrap();

        let first = IndexerLock::acquire(&data_dir, temp.path(), "full").unwrap();
        let second = IndexerLock::acquire(&data_dir, temp.path(), "watch");

        assert!(second.is_err());
        drop(first);
        assert!(IndexerLock::acquire(&data_dir, temp.path(), "watch").is_ok());
    }

    #[test]
    fn recovers_stale_lock_and_rewrites_status() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join(".codeatlas");
        std::fs::create_dir_all(&data_dir).unwrap();
        let stale_pid = u32::MAX - 1;
        let stale_lock = data_dir.join("indexer.lock");
        let stale_status = status_path(&data_dir, stale_pid);
        let stale_json = serde_json::json!({
            "pid": stale_pid,
            "mode": "watch",
            "workspace_root": temp.path().display().to_string(),
            "acquired_at": "2026-04-21T00:00:00Z"
        });
        std::fs::write(&stale_lock, serde_json::to_string(&stale_json).unwrap()).unwrap();
        std::fs::write(&stale_status, "{}").unwrap();

        let lock = IndexerLock::acquire(&data_dir, temp.path(), "incremental").unwrap();
        assert!(data_dir.join("indexer.lock").exists());
        assert!(!stale_status.exists());
        drop(lock);
    }
}
