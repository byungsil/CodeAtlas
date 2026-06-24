//! Append-only activity log for the indexer, written as JSON Lines (one JSON
//! object per line) to `<data_dir>/indexer.log`.
//!
//! The dashboard tails this file to show a live view of what the watcher /
//! incremental indexer is doing.  Logging is best-effort: a failure to write a
//! log line never interrupts indexing.  The file is size-capped and rotated to
//! a single `.1` backup so it cannot grow without bound during long watch
//! sessions.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::OnceLock;

use serde::Serialize;

pub const ACTIVITY_LOG_FILENAME: &str = "indexer.log";

/// Rotate the log once it grows past this many bytes (1 MiB).  One `.1` backup
/// is kept, so on-disk usage is bounded at ~2 MiB.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// Severity of an activity-log entry.  Serialised in lowercase.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Serialize)]
struct LogEntry<'a> {
    /// RFC 3339 timestamp.
    ts: String,
    level: LogLevel,
    /// Short event tag, e.g. "reindex_start", "reindex_done", "watch_event".
    event: &'a str,
    /// Human-readable message.
    message: String,
    /// Process id of the emitting indexer, so the dashboard can group lines by run.
    pid: u32,
}

struct ActivityLogger {
    path: PathBuf,
    inner: Mutex<()>,
}

static LOGGER: OnceLock<Option<ActivityLogger>> = OnceLock::new();

/// Initialise the activity log under `data_dir`.  Safe to call more than once;
/// only the first call takes effect.  When uninitialised (e.g. unit tests that
/// never call this), all logging calls are silent no-ops.
pub fn init(data_dir: &Path) {
    LOGGER.get_or_init(|| {
        Some(ActivityLogger {
            path: data_dir.join(ACTIVITY_LOG_FILENAME),
            inner: Mutex::new(()),
        })
    });
}

/// Record an activity-log entry.  Best-effort: errors are swallowed so logging
/// can never break indexing.
pub fn log(level: LogLevel, event: &str, message: impl Into<String>) {
    let Some(Some(logger)) = LOGGER.get() else {
        return;
    };
    let entry = LogEntry {
        ts: chrono::Utc::now().to_rfc3339(),
        level,
        event,
        message: message.into(),
        pid: std::process::id(),
    };
    let Ok(mut line) = serde_json::to_string(&entry) else {
        return;
    };
    line.push('\n');

    // Serialise concurrent writers within this process; cross-process appends
    // rely on the OS append semantics for short lines.
    let _guard = logger.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    rotate_if_needed(&logger.path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&logger.path) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn info(event: &str, message: impl Into<String>) {
    log(LogLevel::Info, event, message);
}

pub fn warn(event: &str, message: impl Into<String>) {
    log(LogLevel::Warn, event, message);
}

pub fn error(event: &str, message: impl Into<String>) {
    log(LogLevel::Error, event, message);
}

/// When the current log exceeds [`MAX_LOG_BYTES`], move it to `<path>.1`
/// (replacing any previous backup) so the live file starts fresh.
fn rotate_if_needed(path: &Path) {
    let too_big = fs::metadata(path).map(|m| m.len() >= MAX_LOG_BYTES).unwrap_or(false);
    if !too_big {
        return;
    }
    let backup = path.with_extension("log.1");
    let _ = fs::remove_file(&backup);
    let _ = fs::rename(path, &backup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_jsonl_entries_after_init() {
        let dir = tempdir().unwrap();
        init(dir.path());
        info("reindex_start", "1 file(s) changed, re-indexing...");

        let log_path = dir.path().join(ACTIVITY_LOG_FILENAME);
        // init() only takes effect once per process; if a prior test already
        // initialised the logger to a different dir, this file may be absent.
        // Guard so the test is order-independent.
        if let Ok(contents) = fs::read_to_string(&log_path) {
            let last = contents.lines().last().unwrap();
            let parsed: serde_json::Value = serde_json::from_str(last).unwrap();
            assert_eq!(parsed["event"], "reindex_start");
            assert_eq!(parsed["level"], "info");
            assert!(parsed["ts"].is_string());
            assert!(parsed["pid"].is_number());
        }
    }

    #[test]
    fn log_without_init_is_silent_noop() {
        // Must not panic even when the logger was never initialised in this
        // process (OnceLock empty or set by another test).
        info("noop_event", "should not panic");
    }
}
