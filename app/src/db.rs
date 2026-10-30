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
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_cv_match", include_str!("../migrations/0001_cv_match.sql")),
    ("0002_documents", include_str!("../migrations/0002_documents.sql")),
];

/// What a copy of the database made before a migration is called after: the database's own file
/// name, then this, then the migration's name ("recruiter.sqlite.before-0002_documents").
const BACKUP_SUFFIX: &str = ".before-";

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
    migrate(&db, Some(path)).await?;
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
    migrate(&db, None).await?;
    Ok(db)
}

/// Applies the migrations schema_migrations doesn't list yet, each in its own transaction. When
/// the database at `path` already has data, a copy of it is made first, so a migration that goes
/// wrong is undone by putting the copy back.
///
/// These queries aren't checked at compile time: schema_migrations is made here, not by a
/// migration, so the database the checks run against (made from migrations/) doesn't have it.
async fn migrate(db: &SqlitePool, path: Option<&Path>) -> sqlx::Result<()> {
    db.execute(sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
          name TEXT PRIMARY KEY,
          applied_at INTEGER NOT NULL
        )
        "#,
    ))
    .await?;
    // Whether the database had migrations applied before this start, so has data to lose; one a
    // migration of this start made doesn't.
    let had_migrations: bool = sqlx::query_scalar(r#"SELECT EXISTS (SELECT 1 FROM schema_migrations)"#)
        .fetch_one(db)
        .await?;
    for (name, sql) in MIGRATIONS {
        let applied: bool = sqlx::query_scalar(r#"SELECT EXISTS (SELECT 1 FROM schema_migrations WHERE name = ?1)"#)
            .bind(name)
            .fetch_one(db)
            .await?;
        if applied {
            continue;
        }
        if let Some(path) = path.filter(|_| had_migrations) {
            back_up_before(db, path, name).await?;
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

/// Copies the database at `path` next to it before `migration` runs, unless that copy is
/// already there (a run of the same migration that failed made it, from the data as it was).
async fn back_up_before(db: &SqlitePool, path: &Path, migration: &str) -> sqlx::Result<()> {
    let backup = backup_path(path, migration);
    if backup.exists() {
        return Ok(());
    }
    // VACUUM INTO writes one consistent copy, with what the write-ahead log holds too.
    sqlx::query(r#"VACUUM INTO ?1"#)
        .bind(backup.to_string_lossy().into_owned())
        .execute(db)
        .await?;
    eprintln!("db: copied the database to {} before {migration}", backup.display());
    Ok(())
}

/// "recruiter.sqlite.before-0002_documents", next to the database.
fn backup_path(path: &Path, migration: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(BACKUP_SUFFIX);
    name.push(migration);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_migration_runs_once() {
        let db = open_in_memory().await.unwrap();
        migrate(&db, None).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(count as usize, MIGRATIONS.len());
    }

    /// A database as it was with only the first migration: a match with an application and a
    /// résumé written as fields, and one with neither.
    const BEFORE_DOCUMENTS: &str = r#"
        INSERT INTO trongate_users (id) VALUES (1);
        INSERT INTO cv_matches (id, trongate_user_id, job_title, job_text, cv_text, score, points, max_points,
                                tag, application_text, application_written_at, resume_text, resume_written_at,
                                created_at)
        VALUES (7, 1, 'PHP developer', 'post', 'cv', 0.81, 8, 10, 'good', 'Dear Acme', 200, 'Me', 300, 100),
               (8, 1, 'Mechanic', 'post', 'cv', 0.1, 1, 10, 'poor', NULL, NULL, NULL, NULL, 100);
        INSERT INTO cv_match_resumes (cv_match_id, name, intro) VALUES (7, 'Me', 'Hello');
        INSERT INTO cv_match_resume_entries (id, cv_match_id, section, title) VALUES (31, 7, 'experience', 'Developer');
        INSERT INTO cv_match_resume_lines (cv_match_id, entry_id, kind, text)
        VALUES (7, 31, 'bullet', 'Built things'), (7, NULL, 'skill', 'PHP');
    "#;

    /// Applies the first migration only and records it, as an older app did.
    async fn first_migration_only(db: &SqlitePool) {
        let (name, sql) = MIGRATIONS[0];
        db.execute(sqlx::raw_sql(
            r#"CREATE TABLE schema_migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)"#,
        ))
        .await
        .unwrap();
        db.execute(sqlx::raw_sql(sql)).await.unwrap();
        sqlx::query(r#"INSERT INTO schema_migrations (name, applied_at) VALUES (?1, 0)"#)
            .bind(name)
            .execute(db)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn the_documents_move_out_of_cv_matches_with_the_resume_fields() {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let db = SqlitePoolOptions::new()
            .max_connections(IN_MEMORY_CONNECTIONS)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .unwrap();
        first_migration_only(&db).await;
        db.execute(sqlx::raw_sql(BEFORE_DOCUMENTS)).await.unwrap();
        migrate(&db, None).await.unwrap();

        let documents: Vec<(i64, i64, String, String, i64)> =
            sqlx::query_as(r#"SELECT id, cv_match_id, kind, text, written_at FROM documents ORDER BY id"#)
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(
            documents,
            [
                (1, 7, "application".to_string(), "Dear Acme".to_string(), 200),
                (2, 7, "resume".to_string(), "Me".to_string(), 300),
            ]
        );
        let resume = crate::cv_match::cv_match_model::resume_fields(&db, 7)
            .await
            .unwrap()
            .expect("the résumé keeps its fields");
        assert_eq!(
            (resume.name.as_str(), resume.skills.as_slice()),
            ("Me", ["PHP".to_string()].as_slice())
        );
        assert_eq!(resume.experience[0].bullets, ["Built things"]);
        let columns: Vec<String> =
            sqlx::query_scalar(r#"SELECT name FROM pragma_table_info('cv_matches') WHERE name LIKE '%_text'"#)
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(columns, ["job_text", "cv_text"]);
        let leftovers: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM sqlite_schema WHERE name LIKE '%_before%'"#)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(leftovers, 0);
        let broken: Vec<(String,)> = sqlx::query_as(r#"SELECT "table" FROM pragma_foreign_key_check"#)
            .fetch_all(&db)
            .await
            .unwrap();
        assert!(broken.is_empty(), "{broken:?}");
    }

    #[tokio::test]
    async fn a_database_with_data_is_copied_before_a_migration() {
        let folder = std::env::temp_dir().join(format!("recruiter-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("recruiter.sqlite");
        let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let old = SqlitePoolOptions::new().connect_with(options).await.unwrap();
        first_migration_only(&old).await;
        old.execute(sqlx::raw_sql(BEFORE_DOCUMENTS)).await.unwrap();
        old.close().await;

        open(&path).await.unwrap().close().await;
        let backup = backup_path(&path, "0002_documents");
        let copy = SqlitePoolOptions::new()
            .connect_with(SqliteConnectOptions::new().filename(&backup))
            .await
            .unwrap();
        let application: Option<String> = sqlx::query_scalar(r#"SELECT application_text FROM cv_matches WHERE id = 7"#)
            .fetch_one(&copy)
            .await
            .unwrap();
        copy.close().await;
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(application.as_deref(), Some("Dear Acme"));
    }

    #[tokio::test]
    async fn a_new_database_is_not_copied() {
        let folder = std::env::temp_dir().join(format!("recruiter-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join("recruiter.sqlite");
        open(&path).await.unwrap().close().await;
        let copied = MIGRATIONS.iter().any(|(name, _)| backup_path(&path, name).exists());
        let _ = std::fs::remove_dir_all(&folder);
        assert!(!copied);
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
