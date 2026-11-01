//! Whether a job scan runs now: the scan holds a lock for as long as it runs, from listing the
//! posts to the last match, whichever program runs it (Scan now in the app, or the scheduled
//! `jobfinder scan` in a process of its own).
//!
//! On a database file the lock is the operating system's lock on a file beside it
//! ("jobfinder.sqlite.scan.lock"), which the system lets go of when the program that holds it
//! ends, however it ends. A scan cut off by the app being closed so never blocks the next one,
//! and never shows as running. An in-memory database (tests) is only ever seen by its own
//! program, so a flag in its Context does.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::context::Context;

/// What is added to the database file's name for the lock's file.
pub const LOCK_SUFFIX: &str = ".scan.lock";

/// How many times a scan tries to take the lock: is_held() takes it for a moment to look, so a
/// scan that starts in that moment tries again.
const HOLD_TRIES: u32 = 5;

/// How long a scan waits between two tries, in milliseconds.
const HOLD_RETRY_MS: u64 = 100;

/// The lock, held until it is dropped.
pub struct HeldScan<'a> {
    /// The locked file, on a database file. The system unlocks it when it is closed.
    _file: Option<File>,
    /// The flag set, on an in-memory database.
    flag: Option<&'a AtomicBool>,
}

impl Drop for HeldScan<'_> {
    fn drop(&mut self) {
        if let Some(flag) = self.flag {
            flag.store(false, Ordering::Release);
        }
    }
}

/// The lock's file, for a database file ("" is in memory: none).
fn lock_file(database: &str) -> Option<PathBuf> {
    (!database.is_empty()).then(|| PathBuf::from(format!("{database}{LOCK_SUFFIX}")))
}

/// Takes the lock, or None when a scan holds it.
fn try_hold(context: &Context) -> std::io::Result<Option<HeldScan<'_>>> {
    let Some(path) = lock_file(&context.database.path) else {
        let flag = &context.scan_in_memory;
        if flag
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        // Made only once taken: dropping one lets go of the flag.
        return Ok(Some(HeldScan {
            _file: None,
            flag: Some(flag),
        }));
    };
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(HeldScan {
            _file: Some(file),
            flag: None,
        })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

/// Takes the lock for a scan, or None when another scan holds it.
pub async fn hold(context: &Context) -> std::io::Result<Option<HeldScan<'_>>> {
    for _ in 1..HOLD_TRIES {
        if let Some(held) = try_hold(context)? {
            return Ok(Some(held));
        }
        tokio::time::sleep(Duration::from_millis(HOLD_RETRY_MS)).await;
    }
    try_hold(context)
}

/// Whether a scan holds the lock now.
pub fn is_held(context: &Context) -> std::io::Result<bool> {
    Ok(try_hold(context)?.is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn context(database: &str) -> Context {
        let db = crate::db::open_in_memory().await.unwrap();
        let mut context = Context::new(
            db,
            std::sync::Arc::new(crate::taxonomy::Taxonomy::load()),
            1,
            crate::account::key_vault::KeyVault::in_memory(),
        );
        context.database.path = database.to_string();
        context
    }

    #[tokio::test]
    async fn one_scan_holds_the_lock_until_it_ends() {
        let folder = std::env::temp_dir().join(format!("jobfinder-scan-lock-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("jobfinder.sqlite");
        for database in ["", file.to_str().unwrap()] {
            let context = context(database).await;
            assert!(!is_held(&context).unwrap(), "{database}");
            let held = hold(&context).await.unwrap();
            assert!(held.is_some());
            assert!(is_held(&context).unwrap(), "{database}");
            assert!(hold(&context).await.unwrap().is_none(), "{database}");
            drop(held);
            assert!(!is_held(&context).unwrap(), "{database}");
        }
        assert_eq!(lock_file(""), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
