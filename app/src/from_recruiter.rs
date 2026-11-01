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

use crate::{app_log, cleanup, db};

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
const FORMER_LOCKS: [&str; 3] = ["recruiter.log.lock", "recruiter.sqlite-lock", "recruiter-folder.lock"];

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
    // The former file is gone after the move, so its full path is made from its folder's.
    let users_data_folder = std::fs::canonicalize(db::users_data_folder()).unwrap_or_else(|_| db::users_data_folder());
    former_path_in(&users_data_folder).display().to_string()
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
    let mut moved = false;
    if former.exists() {
        if database.exists() {
            return Ok(Moved::BothThere {
                former: former_folder.to_path_buf(),
            });
        }
        // A former Recruiter that runs on it holds its lock (cleanup::hold).
        if cleanup::hold(former, cleanup::Hold::Cleanup).is_err() {
            return Err(format!(
                "Recruiter, this app's former name, is still running on {}. Close it, then start JobFinder again, which moves its data to {}.",
                former.display(),
                folder.display()
            ));
        }
        move_folder(former_folder, folder)?;
        moved = true;
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

fn folder_of(file: &Path) -> &Path {
    file.parent().unwrap_or(Path::new("."))
}

/// Moves former_folder to folder: renamed whole when folder isn't there, else entry by entry
/// (an entry folder already has stays in former_folder, which is removed once empty).
fn move_folder(former_folder: &Path, folder: &Path) -> Result<(), String> {
    let couldnt = |e: std::io::Error| {
        format!(
            "Recruiter's data folder, {}, couldn't be moved to {}: {e}. Close Recruiter if it is open, then start JobFinder again.",
            former_folder.display(),
            folder.display()
        )
    };
    if !folder.exists() {
        if let Some(parent) = folder.parent() {
            std::fs::create_dir_all(parent).map_err(couldnt)?;
        }
        return std::fs::rename(former_folder, folder).map_err(couldnt);
    }
    for entry in std::fs::read_dir(former_folder).map_err(couldnt)? {
        let entry = entry.map_err(couldnt)?;
        let target = folder.join(entry.file_name());
        if !target.exists() {
            std::fs::rename(entry.path(), target).map_err(couldnt)?;
        }
    }
    // remove_dir only removes an empty folder; one with entries left stays.
    let _ = std::fs::remove_dir(former_folder);
    Ok(())
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
