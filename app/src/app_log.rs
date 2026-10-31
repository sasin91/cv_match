//! The app's log: every line the app writes to the console about what it did or what failed also
//! goes to recruiter.log next to the database, with the time. A window build has no console, and
//! the job scan runs from Task Scheduler or a systemd timer without one, so the log file is where
//! a failure can be found after the fact.
//!
//! The file is opened once, at start-up (open); before that, and in tests, lines only go to the
//! console. A log over MAX_LOG_BYTES is moved to recruiter.old.log when the app starts, replacing
//! the old one, so the two together stay under twice that.
//!
//! log_line! is eprintln! plus the file; say! is println! plus the file.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The log's file name, in the database's folder.
pub const FILE: &str = "recruiter.log";
/// The log before the last start-up that found it too big.
pub const OLD_FILE: &str = "recruiter.old.log";
/// How big the log may grow before a start-up moves it aside: 5 MB, months of normal use.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// The open log file. Set once by open and only appended to after; a Mutex so lines from
/// different threads don't interleave.
static LOG: OnceLock<Mutex<File>> = OnceLock::new();

/// Opens folder's recruiter.log for this run, moving it to recruiter.old.log first when it is
/// over MAX_LOG_BYTES, and answers its path. A second call keeps the first file.
pub fn open(folder: &Path) -> std::io::Result<PathBuf> {
    let (path, file) = open_file(folder, MAX_LOG_BYTES)?;
    let _ = LOG.set(Mutex::new(file));
    Ok(path)
}

/// folder's log, opened to append, after moving it aside when it is over max_bytes.
fn open_file(folder: &Path, max_bytes: u64) -> std::io::Result<(PathBuf, File)> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(FILE);
    let too_big = std::fs::metadata(&path).is_ok_and(|meta| meta.len() > max_bytes);
    if too_big {
        std::fs::rename(&path, folder.join(OLD_FILE))?;
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((path, file))
}

/// Writes line to stderr and, once open, to the log.
pub fn error(line: &str) {
    eprintln!("{line}");
    to_file(line);
}

/// Writes line to stdout and, once open, to the log.
pub fn say(line: &str) {
    println!("{line}");
    to_file(line);
}

/// "2026-10-31 23:40:12 line" at the end of the log, when it is open. A failed write is let go:
/// the line is on the console already, and there is nowhere else to say it.
fn to_file(line: &str) {
    let Some(log) = LOG.get() else {
        return;
    };
    let stamped = stamped(&chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), line);
    if let Ok(mut file) = log.lock() {
        let _ = file.write_all(stamped.as_bytes());
    }
}

/// The line as the log keeps it: after the time, each of its lines indented under the first.
fn stamped(time: &str, line: &str) -> String {
    format!("{time} {}\n", line.trim_end().replace('\n', "\n    "))
}

/// eprintln!, also written to the log file (app_log).
#[macro_export]
macro_rules! log_line {
    ($($arg:tt)*) => {
        $crate::app_log::error(&format!($($arg)*))
    };
}

/// println!, also written to the log file (app_log).
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::app_log::say(&format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logged_line_has_its_time_and_keeps_its_lines_together() {
        assert_eq!(
            stamped("2026-10-31 23:40:12", "Couldn't open x"),
            "2026-10-31 23:40:12 Couldn't open x\n"
        );
        assert_eq!(
            stamped("2026-10-31 23:40:12", "usage: a\n  b\n"),
            "2026-10-31 23:40:12 usage: a\n      b\n"
        );
    }

    #[test]
    fn a_log_over_its_size_is_moved_aside_at_start_up() {
        let folder = std::env::temp_dir().join(format!("recruiter-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let (path, mut file) = open_file(&folder, 10).unwrap();
        file.write_all(b"short\n").unwrap();
        drop(file);
        open_file(&folder, 10).unwrap();
        assert!(!folder.join(OLD_FILE).exists(), "6 bytes is under 10");
        std::fs::write(&path, "more than ten bytes\n").unwrap();
        open_file(&folder, 10).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join(OLD_FILE)).unwrap(),
            "more than ten bytes\n"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
