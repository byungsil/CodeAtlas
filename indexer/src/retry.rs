use std::io;
use std::path::Path;
use std::thread;
use std::time::Duration;

use rusqlite::ErrorCode;

use crate::storage;

pub const IO_RETRY_BACKOFF_MS: &[u64] = &[0, 100, 250, 500];

pub fn retry_io<T, F>(operation: &str, mut action: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut last_error = None;

    for (attempt, delay_ms) in IO_RETRY_BACKOFF_MS.iter().enumerate() {
        if *delay_ms > 0 {
            thread::sleep(Duration::from_millis(*delay_ms));
        }

        match action() {
            Ok(value) => return Ok(value),
            Err(err) if should_retry_io_error(&err) && attempt + 1 < IO_RETRY_BACKOFF_MS.len() => {
                last_error = Some(err);
            }
            Err(err) => {
                return Err(io::Error::new(
                    err.kind(),
                    format!("{} failed: {}", operation, err),
                ));
            }
        }
    }

    let err = last_error.unwrap_or_else(|| io::Error::other("retry failed without an error"));
    Err(io::Error::new(
        err.kind(),
        format!("{} failed after retries: {}", operation, err),
    ))
}

pub fn should_retry_io_error(err: &io::Error) -> bool {
    matches!(err.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock)
}

pub fn open_database_with_retry(path: &Path, operation: &str) -> Result<storage::Database, String> {
    let mut last_error = None;

    for (attempt, delay_ms) in IO_RETRY_BACKOFF_MS.iter().enumerate() {
        if *delay_ms > 0 {
            thread::sleep(Duration::from_millis(*delay_ms));
        }

        match storage::Database::open(path) {
            Ok(db) => return Ok(db),
            Err(err) if should_retry_sqlite_open(&err) && attempt + 1 < IO_RETRY_BACKOFF_MS.len() => {
                last_error = Some(err.to_string());
            }
            Err(err) => {
                return Err(format!("{} failed: {}", operation, err));
            }
        }
    }

    Err(format!(
        "{} failed after retries: {}",
        operation,
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

pub fn should_retry_sqlite_open(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(code, _) if code.code == ErrorCode::CannotOpen || code.code == ErrorCode::DatabaseBusy
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn retry_io_retries_permission_denied_then_succeeds() {
        let call_count = AtomicUsize::new(0);
        let result = retry_io("test op", || {
            let n = call_count.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert!(call_count.load(Ordering::SeqCst) >= 3);
    }

    #[test]
    fn retry_io_does_not_retry_non_retryable_errors() {
        let call_count = AtomicUsize::new(0);
        let result: io::Result<i32> = retry_io("test op", || {
            call_count.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::new(io::ErrorKind::NotFound, "missing"))
        });
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }
}
