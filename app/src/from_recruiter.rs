//! What JobFinder takes over from when it was called Recruiter, so nothing is lost by the new
//! name. At each start on the default database:
//!
//! - the former data folder (%APPDATA%\Recruiter, ~/.local/share/Recruiter on Linux) is moved to
//!   the new one, with the database, its log and the downloads in it, and the files named after
//!   the former name (FORMER_FILES) take the new names;
//! - the job scan's former schedule is moved to the new one (ScanSchedule::take_over_former).
//!
//! And at each start on any database, the API keys kept under the former name in Credential
//! Manager move to the new one (KeyVault::take_over_former_keys). Each step is done only when
//! there is something left to take over, so later starts find nothing and change nothing.

use std::path::{Path, PathBuf};

use crate::{app_log, cleanup, db, extension, laya, llama};

/// The data folder's name while the app was called Recruiter.
pub const FORMER_FOLDER: &str = "Recruiter";

/// The database's file name while the app was called Recruiter.
pub const FORMER_FILE: &str = "recruiter.sqlite";

/// SQLite's files beside a database, by what follows its name.
const DATABASE_SUFFIXES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

/// The former log files, each with the name it has now.
const FORMER_LOGS: [(&str, &str); 2] = [
    ("recruiter.log", app_log::FILE),
    ("recruiter.old.log", app_log::OLD_FILE),
];

/// The lock files a former Recruiter left; nothing holds them once it is closed.
const FORMER_LOCKS: [&str; 3] = ["recruiter.log.lock", "recruiter.sqlite-lock", FORMER_FOLDER_LOCK];

/// What the data folder's move did.
#[derive(Debug, PartialEq)]
pub enum Moved {
    /// Nothing to move: no former data, or a database other than the default one.
    Nothing,
    /// The former data folder (or the files in it named the former way) moved to the new one.
    Moved { from: PathBuf, to: PathBuf },
    /// Both folders hold a database, so the former one is left where it is.
    BothThere { former: PathBuf },
}

/// The default database's path while the app was called Recruiter.
pub fn former_default_path() -> PathBuf {
    former_path_in(&db::users_data_folder())
}

fn former_path_in(users_data_folder: &Path) -> PathBuf {
    users_data_folder.join(FORMER_FOLDER).join(FORMER_FILE)
}

/// Whether the former default database was left where it is (Moved::BothThere) while database
/// is the default one: then what the former app kept for it (keys, schedule) stays with it.
pub fn former_left_in_place(database: &Path) -> bool {
    is_default(database) && former_default_path().exists()
}

/// Whether database is the default one, the only one whose folder had the former name.
pub fn is_default(database: &Path) -> bool {
    database == db::default_path()
}

/// The full path the former app named database's keys by: the former default database's, for
/// the default database (it was there before the move), else database's own.
pub fn former_key_database(database: &Path) -> String {
    if !is_default(database) {
        return db::full_path(database);
    }
    former_default_full_path().display().to_string()
}

/// The former default database's full path as the former app resolved it (canonical, so through
/// any symlink in the users' data folder). The file is gone after the move, so it is made from
/// its folder's. On Windows it starts with \\?\, as std::fs::canonicalize gives it.
fn former_default_full_path() -> PathBuf {
    let users_data_folder = std::fs::canonicalize(db::users_data_folder()).unwrap_or_else(|_| db::users_data_folder());
    former_path_in(&users_data_folder)
}

/// The former default database's path as the former app wrote it into its job scan schedule
/// (db::shown_path).
pub fn former_scheduled_database() -> PathBuf {
    PathBuf::from(db::without_verbatim_prefix(
        former_default_full_path().display().to_string(),
    ))
}

/// Moves the former data folder to database's when database is the default one (see the
/// module). Err says why it couldn't, and then the app mustn't start on an empty database.
pub fn move_data_folder(database: &Path) -> Result<Moved, String> {
    if !is_default(database) {
        return Ok(Moved::Nothing);
    }
    move_data_folder_from(&former_default_path(), database)
}

/// Moves the data folder of former (a database) to database's folder, and renames the files
/// there named the former way.
fn move_data_folder_from(former: &Path, database: &Path) -> Result<Moved, String> {
    let folder = folder_of(database);
    let former_folder = folder_of(former);
    if former.exists() && database.exists() {
        return Ok(Moved::BothThere {
            former: former_folder.to_path_buf(),
        });
    }
    let mut moved = false;
    // Also when a start before was cut off with some of them moved and some not.
    if any_left_to_move(former_folder, folder) {
        moved = move_held(former, folder)?;
    }
    // Also when the folder moved at an earlier start whose renames below didn't all finish.
    moved |= rename_former_files(folder, database)?;
    Ok(if moved {
        Moved::Moved {
            from: former_folder.to_path_buf(),
            to: folder.to_path_buf(),
        }
    } else {
        Moved::Nothing
    })
}

/// Whether any of the app's entries (app_entries) is in former_folder and not yet in folder.
fn any_left_to_move(former_folder: &Path, folder: &Path) -> bool {
    app_entries()
        .iter()
        .any(|name| former_folder.join(name).exists() && !folder.join(name).exists())
}

/// Moves former's folder's app entries (app_entries) to folder while holding the locks a running Recruiter
/// holds: former's own (cleanup::hold) and its folder's FORMER_FOLDER_LOCK, which any Recruiter
/// on a database in that folder holds. Held until the files are moved, so no scheduled
/// `recruiter scan` opens the database meanwhile. Whether anything moved.
fn move_held(former: &Path, folder: &Path) -> Result<bool, String> {
    let still_running = || {
        format!(
            "Recruiter, this app's former name, is still running in {}. Close it, then start JobFinder again, which moves its data to {}.",
            folder_of(former).display(),
            folder.display()
        )
    };
    let former_folder = folder_of(former);
    let held = cleanup::hold(former, cleanup::Hold::Cleanup).map_err(|_| still_running())?;
    let folder_lock_file = former_folder.join(FORMER_FOLDER_LOCK);
    let folder_lock = lock_alone(&folder_lock_file).map_err(|_| still_running())?;
    let mut lock_files = cleanup::lock_files(former).to_vec();
    lock_files.push(folder_lock_file);
    let moving = move_folder(former_folder, folder, &app_entries());
    drop(held);
    drop(folder_lock);
    let former_locks = FORMER_LOCKS.iter().map(|name| former_folder.join(name));
    for lock_file in lock_files.iter().cloned().chain(former_locks) {
        // A lock file is only a name to lock; one that stays does no harm.
        let _ = std::fs::remove_file(lock_file);
    }
    // remove_dir only removes an empty folder; one with entries left stays.
    let _ = std::fs::remove_dir(former_folder);
    moving
}

/// The folder lock's file name while the app was called Recruiter (cleanup::lock_files).
const FORMER_FOLDER_LOCK: &str = "recruiter-folder.lock";

/// Locks path alone (made when it isn't there), held until the file is dropped; Err(()) when
/// another process holds it. A file that can't be made or locked at all doesn't stop it.
fn lock_alone(path: &Path) -> Result<Option<std::fs::File>, ()> {
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
    else {
        return Ok(None);
    };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(()),
        Err(std::fs::TryLockError::Error(_)) => Ok(None),
    }
}

fn folder_of(file: &Path) -> &Path {
    file.parent().unwrap_or(Path::new("."))
}

/// The entries of the former data folder that are the default database's and the app's own:
/// the database with SQLite's files, the logs, and the folders the app writes beside it. Other
/// entries (a database that --database named there, say) stay where their schedule and keys
/// still find them.
fn app_entries() -> Vec<String> {
    let databases = DATABASE_SUFFIXES.iter().map(|suffix| format!("{FORMER_FILE}{suffix}"));
    let logs = FORMER_LOGS.iter().map(|(former, _)| former.to_string());
    let folders = [extension::FOLDER, llama::FOLDER, laya::laya_client::FOLDER].map(String::from);
    databases.chain(logs).chain(folders).collect()
}

/// Moves the entries named names from former_folder to folder, made when it isn't there; an
/// entry folder already has stays. Whether any moved.
fn move_folder(former_folder: &Path, folder: &Path, names: &[String]) -> Result<bool, String> {
    let couldnt = |e: std::io::Error| {
        format!(
            "Recruiter's data folder, {}, couldn't be moved to {}: {e}. Close Recruiter if it is open, then start JobFinder again.",
            former_folder.display(),
            folder.display()
        )
    };
    std::fs::create_dir_all(folder).map_err(couldnt)?;
    let mut moved = false;
    for name in names {
        let (entry, target) = (former_folder.join(name), folder.join(name));
        if entry.exists() && !target.exists() {
            std::fs::rename(entry, target).map_err(couldnt)?;
            moved = true;
        }
    }
    Ok(moved)
}

/// Renames the files in folder that have the former names (FORMER_FILE with its SQLite files,
/// FORMER_LOGS) to database's and the log's names, the database first, and removes
/// FORMER_LOCKS. A file whose new name is taken is left. Whether anything was renamed.
fn rename_former_files(folder: &Path, database: &Path) -> Result<bool, String> {
    let database_name = database
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| db::FILE.to_string());
    let databases = DATABASE_SUFFIXES
        .iter()
        .map(|suffix| (format!("{FORMER_FILE}{suffix}"), format!("{database_name}{suffix}")));
    let logs = FORMER_LOGS
        .iter()
        .map(|(former, now)| (former.to_string(), now.to_string()));
    let mut renamed = false;
    for (former, now) in databases.chain(logs) {
        let (former, now) = (folder.join(former), folder.join(now));
        if !former.exists() || now.exists() {
            continue;
        }
        std::fs::rename(&former, &now).map_err(|e| {
            format!(
                "{} couldn't be renamed to {}: {e}. Close Recruiter if it is open, then start JobFinder again.",
                former.display(),
                now.display()
            )
        })?;
        renamed = true;
    }
    for lock in FORMER_LOCKS {
        // A lock file is only a name to lock; one that stays does no harm.
        let _ = std::fs::remove_file(folder.join(lock));
    }
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test, with the users' data folder's layout in it.
    fn test_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("jobfinder-from-recruiter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn the_former_folder_moves_with_its_files_renamed() {
        let users = test_folder("moves");
        let former = former_path_in(&users);
        let former_folder = former.parent().unwrap();
        write(&former, "database");
        write(&former_folder.join("recruiter.sqlite-wal"), "wal");
        write(&former_folder.join("recruiter.log"), "log");
        write(&former_folder.join("recruiter.log.lock"), "");
        write(&former_folder.join("laya").join("model.onnx"), "laya");
        let database = users.join(db::FOLDER).join(db::FILE);

        let moved = move_data_folder_from(&former, &database).unwrap();

        let folder = users.join(db::FOLDER);
        assert_eq!(
            moved,
            Moved::Moved {
                from: former_folder.to_path_buf(),
                to: folder.clone()
            }
        );
        assert_eq!(read(&database), "database");
        assert_eq!(read(&folder.join("jobfinder.sqlite-wal")), "wal");
        assert_eq!(read(&folder.join(app_log::FILE)), "log");
        assert_eq!(read(&folder.join("laya").join("model.onnx")), "laya");
        assert!(!folder.join("recruiter.log.lock").exists());
        assert!(!former_folder.exists());
        assert_eq!(
            move_data_folder_from(&former, &database).unwrap(),
            Moved::Nothing,
            "a later start has nothing to move"
        );
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn another_database_in_the_former_folder_stays_there() {
        let users = test_folder("custom");
        let former = former_path_in(&users);
        write(&former, "database");
        let custom = former.parent().unwrap().join("custom.sqlite");
        write(&custom, "custom");
        let database = users.join(db::FOLDER).join(db::FILE);

        move_data_folder_from(&former, &database).unwrap();

        assert_eq!(read(&database), "database");
        assert_eq!(read(&custom), "custom");
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn a_new_folder_without_a_database_takes_the_former_files() {
        let users = test_folder("into-existing");
        let former = former_path_in(&users);
        write(&former, "database");
        write(
            &former.parent().unwrap().join("jobnet-joblog").join("manifest.json"),
            "former",
        );
        let folder = users.join(db::FOLDER);
        write(&folder.join("jobnet-joblog").join("manifest.json"), "new");
        let database = folder.join(db::FILE);

        move_data_folder_from(&former, &database).unwrap();

        assert_eq!(read(&database), "database");
        assert_eq!(read(&folder.join("jobnet-joblog").join("manifest.json")), "new");
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn a_database_in_both_folders_leaves_the_former_one() {
        let users = test_folder("both");
        let former = former_path_in(&users);
        write(&former, "former");
        let database = users.join(db::FOLDER).join(db::FILE);
        write(&database, "new");

        assert_eq!(
            move_data_folder_from(&former, &database).unwrap(),
            Moved::BothThere {
                former: former.parent().unwrap().to_path_buf()
            }
        );
        assert_eq!(read(&former), "former");
        assert_eq!(read(&database), "new");
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn a_running_former_app_stops_the_move() {
        let users = test_folder("running");
        let former = former_path_in(&users);
        write(&former, "database");
        let _running = cleanup::hold(&former, cleanup::Hold::Running).unwrap();
        let database = users.join(db::FOLDER).join(db::FILE);

        let why = move_data_folder_from(&former, &database).unwrap_err();

        assert!(why.contains("still running"), "{why}");
        assert!(!database.exists());
        assert_eq!(read(&former), "database");
        drop(_running);
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn a_recruiter_on_another_database_in_the_folder_stops_the_move() {
        let users = test_folder("folder-lock");
        let former = former_path_in(&users);
        write(&former, "database");
        let former_folder_lock = lock_alone(&former.parent().unwrap().join(FORMER_FOLDER_LOCK))
            .unwrap()
            .unwrap();
        let database = users.join(db::FOLDER).join(db::FILE);

        let why = move_data_folder_from(&former, &database).unwrap_err();

        assert!(why.contains("still running"), "{why}");
        assert_eq!(read(&former), "database");
        drop(former_folder_lock);
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn a_wal_left_by_an_interrupted_move_follows_its_database() {
        let users = test_folder("stranded-wal");
        let former = former_path_in(&users);
        write(&former.parent().unwrap().join("recruiter.sqlite-wal"), "wal");
        let folder = users.join(db::FOLDER);
        write(&folder.join(FORMER_FILE), "database");
        let database = folder.join(db::FILE);

        move_data_folder_from(&former, &database).unwrap();

        assert_eq!(read(&database), "database");
        assert_eq!(read(&folder.join("jobfinder.sqlite-wal")), "wal");
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn a_folder_left_by_an_interrupted_move_follows_the_database() {
        let users = test_folder("stranded-llama");
        let former = former_path_in(&users);
        write(
            &former.parent().unwrap().join("llama").join("models").join("m.gguf"),
            "model",
        );
        let folder = users.join(db::FOLDER);
        write(&folder.join(db::FILE), "database");
        let database = folder.join(db::FILE);

        move_data_folder_from(&former, &database).unwrap();

        assert_eq!(read(&folder.join("llama").join("models").join("m.gguf")), "model");
        assert!(!former.parent().unwrap().exists());
        let _ = std::fs::remove_dir_all(&users);
    }

    #[test]
    fn files_left_with_the_former_names_are_renamed_at_a_later_start() {
        let users = test_folder("half-done");
        let folder = users.join(db::FOLDER);
        write(&folder.join(FORMER_FILE), "database");
        let database = folder.join(db::FILE);

        let moved = move_data_folder_from(&former_path_in(&users), &database).unwrap();

        assert!(matches!(moved, Moved::Moved { .. }));
        assert_eq!(read(&database), "database");
        let _ = std::fs::remove_dir_all(&users);
    }
}
