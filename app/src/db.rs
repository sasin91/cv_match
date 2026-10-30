//! The app's SQLite database: one file in the user's data folder, its tables made by the
//! numbered files in migrations/ (applied in order at start-up, each recorded in
//! schema_migrations so it runs once).
//!
//! Queries are written with sqlx's query! macros, checked at compile time against the cached
//! descriptions in .sqlx/ (see the README for updating them after changing a query). Longer
//! ones live in queries/ as .sql files.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Executor, SqlitePool};

/// Every migration, in the order they run. A new one goes at the end, never in between.
const MIGRATIONS: &[(&str, &str)] = &[("0001_cv_match", include_str!("../migrations/0001_cv_match.sql"))];

/// How long a write waits for another connection's write to finish, in milliseconds.
const BUSY_TIMEOUT_MS: u64 = 5_000;

/// Connections the app keeps to its database file: one local user, so a few are plenty.
const MAX_CONNECTIONS: u32 = 4;

/// The one connection an in-memory database has: each connection to `:memory:` is its own
/// database, so the pool must never open a second one or close the first.
const IN_MEMORY_CONNECTIONS: u32 = 1;

/// %APPDATA%\Recruiter\recruiter.sqlite on Windows (~/.local/share/Recruiter on Linux).
pub fn default_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Recruiter")
        .join("recruiter.sqlite")
}

/// The database at path, made if it isn't there, with every migration applied.
pub async fn open(path: &Path) -> sqlx::Result<SqlitePool> {
    if let Some(folder) = path.parent() {
        // A missing folder shows up as the connect's own error just below.
        let _ = std::fs::create_dir_all(folder);
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_millis(BUSY_TIMEOUT_MS))
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
    let db = SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await?;
    migrate(&db).await?;
    Ok(db)
}

/// An in-memory database with every migration applied, for tests.
pub async fn open_in_memory() -> sqlx::Result<SqlitePool> {
    let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
    let db = SqlitePoolOptions::new()
        .min_connections(IN_MEMORY_CONNECTIONS)
        .max_connections(IN_MEMORY_CONNECTIONS)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await?;
    migrate(&db).await?;
    Ok(db)
}

/// Applies the migrations schema_migrations doesn't list yet, each in its own transaction.
///
/// These queries aren't checked at compile time: schema_migrations is made here, not by a
/// migration, so the database the checks run against (made from migrations/) doesn't have it.
async fn migrate(db: &SqlitePool) -> sqlx::Result<()> {
    db.execute(sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
          name TEXT PRIMARY KEY,
          applied_at INTEGER NOT NULL
        )
        "#,
    ))
    .await?;
    for (name, sql) in MIGRATIONS {
        let applied: bool = sqlx::query_scalar(r#"SELECT EXISTS (SELECT 1 FROM schema_migrations WHERE name = ?1)"#)
            .bind(name)
            .fetch_one(db)
            .await?;
        if applied {
            continue;
        }
        let mut tx = db.begin().await?;
        tx.execute(sqlx::raw_sql(*sql)).await?;
        sqlx::query(r#"INSERT INTO schema_migrations (name, applied_at) VALUES (?1, unixepoch())"#)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_migration_runs_once() {
        let db = open_in_memory().await.unwrap();
        migrate(&db).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count as usize, MIGRATIONS.len());
    }

    #[tokio::test]
    async fn deleting_a_match_deletes_what_hangs_on_it() {
        let db = open_in_memory().await.unwrap();
        db.execute(sqlx::raw_sql(
            r#"
            INSERT INTO trongate_users (id) VALUES (1);
            INSERT INTO cv_matches (id, trongate_user_id, job_title, job_text, cv_text, score, points,
                                    max_points, tag, created_at)
            VALUES (7, 1, 'PHP developer', 'post', 'cv', 0.81, 8, 10, 'good', 0);
            INSERT INTO cv_match_items (cv_match_id, criterion, text, verdict, decided_by)
            VALUES (7, 'skills', 'PHP', 'met', 'exact');
            DELETE FROM cv_matches WHERE id = 7;
            "#,
        ))
        .await
        .unwrap();
        let items = sqlx::query_scalar!(r#"SELECT COUNT(*) FROM cv_match_items"#)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(items, 0);
    }
}
