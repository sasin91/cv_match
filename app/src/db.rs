//! The app's SQLite database: one file in the user's data folder, its tables made by the
//! numbered files in migrations/ (applied in order at start-up, each recorded in
//! schema_migrations so it runs once).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// Every migration, in the order they run. A new one goes at the end, never in between.
const MIGRATIONS: &[(&str, &str)] = &[("0001_cv_match", include_str!("../migrations/0001_cv_match.sql"))];

/// How long a write waits for another connection's write to finish, in milliseconds.
const BUSY_TIMEOUT_MS: u64 = 5_000;

/// %APPDATA%\Recruiter\recruiter.sqlite on Windows (~/.local/share/Recruiter on Linux).
pub fn default_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Recruiter")
        .join("recruiter.sqlite")
}

/// The database at path, made if it isn't there, with every migration applied.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(folder) = path.parent() {
        // A missing folder shows up as Connection::open's own error just below.
        let _ = std::fs::create_dir_all(folder);
    }
    let mut db = Connection::open(path)?;
    db.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))?;
    db.pragma_update(None, "journal_mode", "WAL")?;
    prepare(&mut db)?;
    Ok(db)
}

/// An in-memory database with every migration applied, for tests.
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let mut db = Connection::open_in_memory()?;
    prepare(&mut db)?;
    Ok(db)
}

fn prepare(db: &mut Connection) -> rusqlite::Result<()> {
    db.pragma_update(None, "foreign_keys", true)?;
    migrate(db)
}

/// Applies the migrations schema_migrations doesn't list yet, each in its own transaction.
fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           name TEXT PRIMARY KEY,
           applied_at INTEGER NOT NULL
         )",
    )?;
    for (name, sql) in MIGRATIONS {
        let applied: bool = db.query_row(
            "SELECT EXISTS (SELECT 1 FROM schema_migrations WHERE name = ?1)",
            [name],
            |row| row.get(0),
        )?;
        if applied {
            continue;
        }
        let tx = db.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (name, applied_at) VALUES (?1, unixepoch())",
            [name],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// The columns of a table, in their order.
pub fn columns(db: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = db.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?;
    let names = statement.query_map([table], |row| row.get(0))?;
    names.collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_migration_runs_once() {
        let mut db = open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count as usize, MIGRATIONS.len());
    }

    #[test]
    fn deleting_a_match_deletes_what_hangs_on_it() {
        let db = open_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO trongate_users (id) VALUES (1);
             INSERT INTO cv_matches (id, trongate_user_id, job_title, job_text, cv_text, score, points,
                                     max_points, tag, created_at)
             VALUES (7, 1, 'PHP developer', 'post', 'cv', 0.81, 8, 10, 'good', 0);
             INSERT INTO cv_match_items (cv_match_id, criterion, text, verdict, decided_by)
             VALUES (7, 'skills', 'PHP', 'met', 'exact');
             DELETE FROM cv_matches WHERE id = 7;",
        )
        .unwrap();
        let items: i64 = db
            .query_row("SELECT COUNT(*) FROM cv_match_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(items, 0);
    }
}
