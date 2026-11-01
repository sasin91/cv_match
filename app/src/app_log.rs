//! The app's log: every line the app writes to the console about what it did or what failed is
//! also kept where it can be found after the fact. A window build has no console, and the job
//! scan runs from Task Scheduler or a systemd timer without one.
//!
//! - Linux: the systemd journal, as `recruiter` (`journalctl --user -t recruiter`), which keeps
//!   and rotates it. Run by systemd (the scan's timer), the console already goes to the journal,
//!   so nothing is added; without a journal (no systemd) the log file below is used instead.
//! - Windows (and Linux without a journal): recruiter.log next to the database, with the time.
//!   The Event Log was set aside: its event source must be registered by an administrator,
//!   and nothing is installed.
//!
//! The log is opened once, at start-up (open); before that, and in tests, lines only go to the
//! console. A log file over MAX_LOG_BYTES is moved to recruiter.old.log when the app starts,
//! replacing the old one, so the two together stay under twice that. Every process that writes
//! the file (the app, a dev build beside it, the job scan) holds a shared lock on
//! recruiter.log.lock while it runs, and the file is only moved by a start that gets that lock
//! alone: a log another process still writes to is never moved under it.
//!
//! log_line! is eprintln! plus the log; say! is println! plus the log.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The log's file name, in the database's folder.
pub const FILE: &str = "recruiter.log";
/// The log before the last start-up that found it too big.
pub const OLD_FILE: &str = "recruiter.old.log";
/// The lock every writer of the log shares (see the module's comment).
pub const LOCK_FILE: &str = "recruiter.log.lock";
/// How big the log may grow before a start-up moves it aside: 5 MB, months of normal use.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// The open log, and the shared lock that keeps other starts from moving it.
struct Log {
    file: File,
    _lock: File,
}

/// Where the log's lines go besides the console.
enum Sink {
    File(Log),
    #[cfg(target_os = "linux")]
    Journal(journal::Journal),
}

/// Where open sent the log.
#[derive(Debug, PartialEq)]
pub enum Opened {
    /// This file.
    File(PathBuf),
    /// The systemd journal.
    Journal,
    /// Only the console, which systemd already keeps in the journal.
    Console,
}

/// The open log. Set once by open and only written to after; a Mutex so lines from different
/// threads don't interleave.
static LOG: OnceLock<Mutex<Sink>> = OnceLock::new();

/// Opens the log for this run: the journal on Linux, else folder's recruiter.log (moved to
/// recruiter.old.log first when it is over MAX_LOG_BYTES and no other process writes it). A
/// second call keeps the first.
pub fn open(folder: &Path) -> std::io::Result<Opened> {
    #[cfg(target_os = "linux")]
    {
        if journal::keeps_the_console() {
            return Ok(Opened::Console);
        }
        if let Some(journal) = journal::Journal::connect() {
            let _ = LOG.set(Mutex::new(Sink::Journal(journal)));
            return Ok(Opened::Journal);
        }
    }
    let (path, log) = open_file(folder, MAX_LOG_BYTES)?;
    let _ = LOG.set(Mutex::new(Sink::File(log)));
    Ok(Opened::File(path))
}

/// folder's log, opened to append with its shared lock held, after moving it aside when it is
/// over max_bytes and the lock is this process's alone.
fn open_file(folder: &Path, max_bytes: u64) -> std::io::Result<(PathBuf, Log)> {
    std::fs::create_dir_all(folder)?;
    let path = folder.join(FILE);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(folder.join(LOCK_FILE))?;
    let too_big = std::fs::metadata(&path).is_ok_and(|meta| meta.len() > max_bytes);
    // Alone: no other process has the log open, so moving it loses nobody's lines. A start that
    // comes in after the unlock below finds a short log and leaves it.
    if too_big && lock.try_lock().is_ok() {
        std::fs::rename(&path, folder.join(OLD_FILE))?;
        lock.unlock()?;
    }
    lock.lock_shared()?;
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((path, Log { file, _lock: lock }))
}

/// How much a line matters, in syslog's numbers, which the journal keeps.
#[derive(Clone, Copy)]
enum Priority {
    /// Something failed (LOG_ERR).
    Error = 3,
    /// What the app did (LOG_INFO).
    Info = 6,
}

/// Writes line to stderr and, once open, to the log.
pub fn error(line: &str) {
    // Written, not eprintln!: the desktop window has no console, and a write that fails must
    // not end the app.
    let _ = writeln!(std::io::stderr(), "{line}");
    to_log(line, Priority::Error);
}

/// Writes line to stdout and, once open, to the log.
pub fn say(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
    to_log(line, Priority::Info);
}

/// line in the log, when it is open: as a journal entry, or as "2026-10-31 23:40:12 line" at the
/// end of the file. A failed write is let go: the line is on the console already, and there is
/// nowhere else to say it.
fn to_log(line: &str, priority: Priority) {
    let Some(log) = LOG.get() else {
        return;
    };
    let Ok(mut sink) = log.lock() else {
        return;
    };
    match &mut *sink {
        Sink::File(log) => {
            let stamped = stamped(&chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(), line);
            let _ = log.file.write_all(stamped.as_bytes());
        }
        #[cfg(target_os = "linux")]
        Sink::Journal(journal) => journal.send(line, priority as u8),
    }
    #[cfg(not(target_os = "linux"))]
    let _ = priority;
}

/// The line as the log keeps it: after the time, each of its lines indented under the first.
fn stamped(time: &str, line: &str) -> String {
    format!("{time} {}\n", line.trim_end().replace('\n', "\n    "))
}

/// eprintln!, also written to the log (app_log).
#[macro_export]
macro_rules! log_line {
    ($($arg:tt)*) => {
        $crate::app_log::error(&format!($($arg)*))
    };
}

/// println!, also written to the log (app_log).
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => {
        $crate::app_log::say(&format!($($arg)*))
    };
}

/// The systemd journal, written to over its native socket (systemd.journal-fields, "native
/// protocol"), so no library is needed.
#[cfg(target_os = "linux")]
mod journal {
    use std::os::unix::net::UnixDatagram;

    /// Where journald takes entries.
    const SOCKET: &str = "/run/systemd/journal/socket";
    /// The name entries are kept under: `journalctl --user -t recruiter`.
    const IDENTIFIER: &str = "recruiter";
    /// Set by systemd when it keeps the program's console in the journal.
    const JOURNAL_STREAM: &str = "JOURNAL_STREAM";

    pub struct Journal {
        socket: UnixDatagram,
    }

    /// Whether systemd already keeps this program's console in the journal (the scan's timer):
    /// then the console's lines are there already.
    pub fn keeps_the_console() -> bool {
        std::env::var_os(JOURNAL_STREAM).is_some()
    }

    impl Journal {
        /// The journal, or None when there is none to write to.
        pub fn connect() -> Option<Journal> {
            let socket = UnixDatagram::unbound().ok()?;
            socket.connect(SOCKET).ok()?;
            Some(Journal { socket })
        }

        /// Sends line as one entry with priority (syslog's numbers).
        pub fn send(&self, line: &str, priority: u8) {
            let _ = self.socket.send(&entry(line, priority));
        }
    }

    /// The entry's fields. MESSAGE is sent with its length first, so a line with newlines in it
    /// stays one entry.
    pub fn entry(line: &str, priority: u8) -> Vec<u8> {
        let message = line.trim_end().as_bytes();
        let mut entry = format!("SYSLOG_IDENTIFIER={IDENTIFIER}\nPRIORITY={priority}\nMESSAGE\n").into_bytes();
        entry.extend_from_slice(&(message.len() as u64).to_le_bytes());
        entry.extend_from_slice(message);
        entry.push(b'\n');
        entry
    }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn a_journal_entry_keeps_a_line_with_newlines_as_one_message() {
        let entry = journal::entry("usage: a\n  b\n", Priority::Error as u8);
        let mut expected = b"SYSLOG_IDENTIFIER=recruiter\nPRIORITY=3\nMESSAGE\n".to_vec();
        expected.extend_from_slice(&12u64.to_le_bytes());
        expected.extend_from_slice(b"usage: a\n  b");
        expected.push(b'\n');
        assert_eq!(entry, expected);
    }

    #[test]
    fn a_log_over_its_size_is_moved_aside_at_start_up_unless_another_process_writes_it() {
        let folder = std::env::temp_dir().join(format!("recruiter-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let (path, mut log) = open_file(&folder, 10).unwrap();
        log.file.write_all(b"short\n").unwrap();
        drop(log);
        drop(open_file(&folder, 10).unwrap());
        assert!(!folder.join(OLD_FILE).exists(), "6 bytes is under 10");
        // Another process still writes it when it grows too big: it stays where it is.
        let (_, writing) = open_file(&folder, 10).unwrap();
        std::fs::write(&path, "more than ten bytes\n").unwrap();
        drop(open_file(&folder, 10).unwrap());
        assert!(!folder.join(OLD_FILE).exists(), "moved under a writer");
        drop(writing);
        drop(open_file(&folder, 10).unwrap());
        assert_eq!(
            std::fs::read_to_string(folder.join(OLD_FILE)).unwrap(),
            "more than ten bytes\n"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
