//! `recruiter cleanup`: what the app leaves outside its program file, removed. The program itself
//! is never installed, so removing Recruiter is deleting its file; this takes away the rest, after
//! saying what it found:
//!
//! - the job scan's schedule (Task Scheduler's task, or the systemd user timer), which would
//!   otherwise keep running a program that is gone;
//! - the API keys in Credential Manager (or the keyring);
//! - what the app wrote or downloaded next to the database: the Jobnet extension's files,
//!   llama.cpp and its models, Laya's model;
//! - and only when asked again, the database itself, with its -wal and -shm files.
//!
//! The data folder itself goes too when it is the default one (%APPDATA%\Recruiter) and nothing
//! else is left in it; a folder that --database names is never removed, only the app's own
//! files in it.

use std::path::{Path, PathBuf};

use crate::account::key_vault::KeyVault;
use crate::scan_schedule::ScanSchedule;
use crate::{extension, laya, llama};

/// What the cleanup found.
pub struct Found {
    pub database: PathBuf,
    /// The job scan's interval, when it is scheduled.
    pub schedule: Option<u32>,
    /// The schedule couldn't be read.
    pub schedule_error: Option<String>,
    /// The providers whose keys are kept ("" is the key from before providers).
    pub keys: Vec<&'static str>,
    pub keys_error: Option<String>,
    /// The folders the app wrote next to the database that are there.
    pub folders: Vec<PathBuf>,
    /// The database's files that are there.
    pub database_files: Vec<PathBuf>,
}

impl Found {
    /// Whether anything but the database is there to remove.
    pub fn anything_besides_the_database(&self) -> bool {
        self.schedule.is_some() || !self.keys.is_empty() || !self.folders.is_empty()
    }
}

/// The folders the app writes next to database.
pub fn app_folders(database: &Path) -> Vec<PathBuf> {
    let data_folder = database.parent().unwrap_or(Path::new("."));
    [extension::FOLDER, llama::FOLDER, laya::laya_client::FOLDER]
        .iter()
        .map(|name| data_folder.join(name))
        .collect()
}

/// Another Recruiter holds the database's lock: it is running on it.
#[derive(Debug, PartialEq)]
pub struct InUse;

/// The files whose locks a running Recruiter holds for database:
/// - the database's own, its name and -lock beside the file itself, so a link to the database
///   shares it;
/// - its folder's, `recruiter-folder.lock`, since every database in a folder shares the
///   llama, laya and jobnet-joblog folders there.
pub fn lock_files(database: &Path) -> [PathBuf; 2] {
    let file = std::fs::canonicalize(database).unwrap_or_else(|_| database.to_path_buf());
    let mut name = file.into_os_string();
    name.push("-lock");
    let folder = database
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    [PathBuf::from(name), folder.join(FOLDER_LOCK)]
}

/// The folder lock's file name.
const FOLDER_LOCK: &str = "recruiter-folder.lock";

/// How a lock on the database is held.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hold {
    /// The app while it runs: any number of apps share it (a dev build beside the release one).
    Running,
    /// The cleanup: alone, so it starts only when no app runs on the database or another one in
    /// its folder, and no app starts there while it runs.
    Cleanup,
}

/// The locks hold() took, held until this is dropped.
pub struct Held {
    _files: Vec<std::fs::File>,
}

/// Takes database's locks (lock_files). A lock file that can't be made or locked at all (a
/// read-only folder, say) is skipped, which doesn't stop either.
pub fn hold(database: &Path, how: Hold) -> Result<Held, InUse> {
    if let Some(folder) = database.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let mut files = Vec::new();
    for path in lock_files(database) {
        let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
        else {
            continue;
        };
        let locked = match how {
            Hold::Running => file.try_lock_shared(),
            Hold::Cleanup => file.try_lock(),
        };
        match locked {
            Ok(()) => files.push(file),
            Err(std::fs::TryLockError::WouldBlock) => return Err(InUse),
            Err(std::fs::TryLockError::Error(_)) => {}
        }
    }
    Ok(Held { _files: files })
}

/// The database's own files: the file, and SQLite's -wal, -shm and -journal beside it (the lock
/// file goes after the cleanup lets go of it).
pub fn database_files(database: &Path) -> Vec<PathBuf> {
    let mut files = vec![database.to_path_buf()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = database.as_os_str().to_owned();
        name.push(suffix);
        files.push(PathBuf::from(name));
    }
    files
}

/// Looks for what the app left for database, whose keys vault keeps for user_id.
pub async fn find(database: &Path, schedule: &ScanSchedule, vault: &KeyVault, user_id: i64) -> Found {
    let (schedule, schedule_error) = match schedule.interval_hours().await {
        Ok(hours) => (hours, None),
        Err(why) => (None, Some(why)),
    };
    let (keys, keys_error) = match vault.kept(user_id) {
        Ok(keys) => (keys, None),
        Err(why) => (Vec::new(), Some(why)),
    };
    Found {
        database: database.to_path_buf(),
        schedule,
        schedule_error,
        keys,
        keys_error,
        folders: app_folders(database)
            .into_iter()
            .filter(|folder| folder.exists())
            .collect(),
        database_files: database_files(database)
            .into_iter()
            .filter(|file| file.exists())
            .collect(),
    }
}

/// Removes the schedule, the keys and the app's folders; what couldn't be removed, said why.
pub async fn remove_besides_the_database(
    found: &Found,
    schedule: &ScanSchedule,
    vault: &KeyVault,
    user_id: i64,
) -> Vec<String> {
    let mut failed = Vec::new();
    if found.schedule.is_some()
        && let Err(why) = schedule.remove().await
    {
        failed.push(format!("The job scan's schedule: {why}"));
    }
    for provider in &found.keys {
        if let Err(why) = vault.delete(user_id, provider) {
            failed.push(format!("The {} key: {why}", key_name(provider)));
        }
    }
    for folder in &found.folders {
        if let Err(e) = std::fs::remove_dir_all(folder) {
            failed.push(format!("{}: {e}", folder.display()));
        }
    }
    failed
}

/// Removes the database's files; what couldn't be removed, said why.
pub fn remove_database(found: &Found) -> Vec<String> {
    found
        .database_files
        .iter()
        .filter_map(|file| {
            std::fs::remove_file(file)
                .err()
                .map(|e| format!("{}: {e}", file.display()))
        })
        .collect()
}

/// Removes the data folder when it is default_folder and empty; whether it did.
pub fn remove_default_folder(database: &Path, default_folder: &Path) -> bool {
    let Some(folder) = database.parent() else {
        return false;
    };
    // remove_dir only removes an empty folder.
    same_folder(folder, default_folder) && std::fs::remove_dir(folder).is_ok()
}

fn same_folder(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// What the cleanup calls a provider's key.
pub fn key_name(provider: &str) -> &'static str {
    crate::account::account_model::PROVIDERS
        .iter()
        .find(|(name, _)| *name == provider)
        .map(|(_, label)| *label)
        .unwrap_or("earlier")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_database_files_are_the_file_and_sqlites_own_beside_it() {
        let files = database_files(Path::new("/data/recruiter.sqlite"));
        assert_eq!(
            files,
            [
                "/data/recruiter.sqlite",
                "/data/recruiter.sqlite-wal",
                "/data/recruiter.sqlite-shm",
                "/data/recruiter.sqlite-journal"
            ]
            .map(PathBuf::from)
        );
    }

    #[test]
    fn running_apps_share_the_lock_and_the_cleanup_needs_it_alone() {
        let folder = std::env::temp_dir().join(format!("recruiter-lock-{}", std::process::id()));
        let database = folder.join("recruiter.sqlite");
        let app = hold(&database, Hold::Running).unwrap();

        let dev_build = hold(&database, Hold::Running).expect("a second app on the same database starts");

        assert_eq!(hold(&database, Hold::Cleanup).err(), Some(InUse));
        drop(app);
        assert_eq!(
            hold(&database, Hold::Cleanup).err(),
            Some(InUse),
            "the dev build still runs"
        );
        drop(dev_build);
        let cleanup = hold(&database, Hold::Cleanup).unwrap();
        assert_eq!(
            hold(&database, Hold::Running).err(),
            Some(InUse),
            "no app starts during the cleanup"
        );
        drop(cleanup);

        // A link to the database shares its lock.
        #[cfg(unix)]
        {
            std::fs::write(&database, "db").unwrap();
            let alias = folder.join("alias.sqlite");
            std::os::unix::fs::symlink(&database, &alias).unwrap();
            let app = hold(&database, Hold::Running).unwrap();

            assert_eq!(hold(&alias, Hold::Cleanup).err(), Some(InUse));
            // Another database in the same folder shares its folders, so its cleanup waits too.
            assert_eq!(hold(&folder.join("test.sqlite"), Hold::Cleanup).err(), Some(InUse));
            drop(app);
            assert!(hold(&folder.join("test.sqlite"), Hold::Cleanup).is_ok());
        }
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn only_the_apps_own_folders_are_removed_from_a_named_folder() {
        assert_eq!(
            app_folders(Path::new("/home/me/recruiter.sqlite")),
            ["/home/me/jobnet-joblog", "/home/me/llama", "/home/me/laya"].map(PathBuf::from)
        );
    }

    #[tokio::test]
    async fn the_cleanup_removes_the_schedule_the_keys_and_the_folders_and_keeps_the_database() {
        let folder = std::env::temp_dir().join(format!("recruiter-cleanup-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("llama/models")).unwrap();
        std::fs::write(folder.join("llama/models/a.gguf"), "x").unwrap();
        let database = folder.join("recruiter.sqlite");
        std::fs::write(&database, "db").unwrap();
        std::fs::create_dir_all(folder.join("mine")).unwrap();

        let schedule = ScanSchedule::in_memory();
        schedule.register(4).await.unwrap();
        let vault = KeyVault::in_memory();
        vault.save(7, "openai", "sk-1").unwrap();

        let found = find(&database, &schedule, &vault, 7).await;
        assert_eq!(found.schedule, Some(4));
        assert_eq!(found.keys, ["openai"]);
        assert_eq!(found.folders, [folder.join("llama")]);
        assert_eq!(found.database_files, std::slice::from_ref(&database));
        assert!(found.anything_besides_the_database());

        assert!(
            remove_besides_the_database(&found, &schedule, &vault, 7)
                .await
                .is_empty()
        );
        assert_eq!(schedule.interval_hours().await.unwrap(), None);
        assert_eq!(vault.key_for(7, "openai").unwrap(), None);
        assert!(!folder.join("llama").exists());
        assert!(database.exists(), "the database stays unless asked again");
        assert!(folder.join("mine").exists(), "nothing else in the folder is touched");

        assert!(remove_database(&found).is_empty());
        assert!(!database.exists());
        assert!(!remove_default_folder(&database, Path::new("/elsewhere")));
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
