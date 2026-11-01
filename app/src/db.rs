//! The app's SQLite database: one file in the user's data folder, its tables made by the
//! numbered files in migrations/ (applied in order at start-up, each recorded in
//! schema_migrations with its text, so it runs once). A database whose migrations aren't these,
//! word for word, was made from another version of the schema, and is refused.
//!
//! Queries are written with sqlx's query! macros, checked at compile time against the cached
//! descriptions in .sqlx/ (see the README for updating them after changing a query). Longer
//! ones live in queries/ as .sql files.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Acquire, Executor, SqliteConnection, SqlitePool};

/// Every migration, in the order they run. A new one goes at the end, never in between.
const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_schema", include_str!("../migrations/0001_schema.sql")),
    (
        "0002_llm_key_per_provider",
        include_str!("../migrations/0002_llm_key_per_provider.sql"),
    ),
    ("0003_job_scan", include_str!("../migrations/0003_job_scan.sql")),
    (
        "0004_match_documents",
        include_str!("../migrations/0004_match_documents.sql"),
    ),
];

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

/// Applies the migrations schema_migrations doesn't list yet, each in its own transaction, after
/// checking that the ones it lists are these.
///
/// Each runs with foreign keys off, as SQLite's way of making a table again asks
/// (https://sqlite.org/lang_altertable.html#otheralter): dropping the old table would otherwise
/// delete every row that hangs on it. Before it commits, every foreign key is checked, so a
/// migration that leaves one dangling is rolled back.
///
/// These queries aren't checked at compile time: schema_migrations is made here, not by a
/// migration, so the database the checks run against (made from migrations/) doesn't have it.
async fn migrate(db: &SqlitePool) -> sqlx::Result<()> {
    db.execute(sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
          name TEXT PRIMARY KEY,
          sql TEXT NOT NULL,
          applied_at INTEGER NOT NULL
        )
        "#,
    ))
    .await?;
    refuse_another_schema(db).await?;
    for (name, sql) in MIGRATIONS {
        let applied: bool = sqlx::query_scalar(r#"SELECT EXISTS (SELECT 1 FROM schema_migrations WHERE name = ?1)"#)
            .bind(name)
            .fetch_one(db)
            .await?;
        if applied {
            continue;
        }
        // The pragma is per connection and ignored inside a transaction, so it is set on the
        // connection the transaction then runs on, and set back after.
        let mut connection = db.acquire().await?;
        connection.execute("PRAGMA foreign_keys = OFF").await?;
        let applied = apply(&mut connection, name, sql).await;
        connection.execute("PRAGMA foreign_keys = ON").await?;
        applied?;
    }
    Ok(())
}

/// One migration in a transaction, committed only when every foreign key still points somewhere.
async fn apply(connection: &mut SqliteConnection, name: &str, sql: &'static str) -> sqlx::Result<()> {
    let mut tx = connection.begin().await?;
    tx.execute(sqlx::raw_sql(sql)).await?;
    let dangling: Vec<(String,)> = sqlx::query_as(r#"SELECT "table" FROM pragma_foreign_key_check"#)
        .fetch_all(&mut *tx)
        .await?;
    if let Some((table,)) = dangling.first() {
        return Err(sqlx::Error::Protocol(format!(
            "migration {name} left {} rows pointing at nothing, first in {table}",
            dangling.len()
        )));
    }
    sqlx::query(r#"INSERT INTO schema_migrations (name, sql, applied_at) VALUES (?1, ?2, unixepoch())"#)
        .bind(name)
        .bind(sql)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

/// Fails when a migration schema_migrations lists isn't one of MIGRATIONS with the same text: the
/// database was made from another version of migrations/ (changed in place while the app is
/// new), so its tables aren't the ones the queries expect. A schema_migrations without the text
/// is from before it was kept, so from another version too.
async fn refuse_another_schema(db: &SqlitePool) -> sqlx::Result<()> {
    let keeps_text: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (SELECT 1 FROM pragma_table_info('schema_migrations') WHERE name = 'sql')"#,
    )
    .fetch_one(db)
    .await?;
    let other: Vec<String> = if keeps_text {
        let applied: Vec<(String, String)> = sqlx::query_as(r#"SELECT name, sql FROM schema_migrations ORDER BY name"#)
            .fetch_all(db)
            .await?;
        applied
            .into_iter()
            .filter(|(name, sql)| !MIGRATIONS.contains(&(name.as_str(), sql.as_str())))
            .map(|(name, _)| name)
            .collect()
    } else {
        sqlx::query_scalar(r#"SELECT name FROM schema_migrations ORDER BY name"#)
            .fetch_all(db)
            .await?
    };
    if other.is_empty() {
        return Ok(());
    }
    Err(sqlx::Error::Configuration(
        format!(
            "the database was made by another version of the app (migration {}); \
             move it aside and start the app again for a new one",
            other.join(", ")
        )
        .into(),
    ))
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

    /// The error that refuses a database made from another schema.
    async fn refusal(db: &SqlitePool) -> String {
        migrate(db).await.unwrap_err().to_string()
    }

    #[tokio::test]
    async fn a_database_from_another_schema_is_refused() {
        let db = open_in_memory().await.unwrap();
        sqlx::query(r#"UPDATE schema_migrations SET sql = 'CREATE TABLE before (id INTEGER)'"#)
            .execute(&db)
            .await
            .unwrap();
        let refused = refusal(&db).await;
        assert!(
            refused.contains("0001_schema") && refused.contains("move it aside"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_database_from_before_the_schema_was_kept_is_refused() {
        let options = SqliteConnectOptions::new().in_memory(true);
        let db = SqlitePoolOptions::new()
            .max_connections(IN_MEMORY_CONNECTIONS)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .unwrap();
        db.execute(sqlx::raw_sql(
            r#"
            CREATE TABLE schema_migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL);
            INSERT INTO schema_migrations (name, applied_at) VALUES ('0001_cv_match', 0);
            "#,
        ))
        .await
        .unwrap();
        assert!(refusal(&db).await.contains("0001_cv_match"));
    }

    #[tokio::test]
    async fn deleting_a_match_deletes_what_hangs_on_it() {
        let db = open_in_memory().await.unwrap();
        db.execute(sqlx::raw_sql(
            r#"
            INSERT INTO trongate_users (id) VALUES (1);
            INSERT INTO matches (id, trongate_user_id, job_title, job_text, score, points,
                                    max_points, tag, created_at)
            VALUES (7, 1, 'PHP developer', 'post', 0.81, 8, 10, 'good', 0);
            INSERT INTO documents (id, kind, text, written_at) VALUES (1, 'cv', 'cv', 0);
            INSERT INTO match_documents (match_id, document_id) VALUES (7, 1);
            INSERT INTO match_items (match_id, criterion, text, verdict, decided_by)
            VALUES (7, 'skills', 'PHP', 'met', 'exact');
            DELETE FROM matches WHERE id = 7;
            "#,
        ))
        .await
        .unwrap();
        let items = sqlx::query_scalar!(r#"SELECT COUNT(*) FROM match_items"#)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(items, 0);
    }

    #[tokio::test]
    async fn a_saved_key_stays_when_keys_become_per_provider() {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let db = SqlitePoolOptions::new()
            .max_connections(IN_MEMORY_CONNECTIONS)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .unwrap();
        let (first_name, first_sql) = MIGRATIONS[0];
        db.execute(sqlx::raw_sql(first_sql)).await.unwrap();
        db.execute(sqlx::raw_sql(
            r#"
            CREATE TABLE schema_migrations (name TEXT PRIMARY KEY, sql TEXT NOT NULL, applied_at INTEGER NOT NULL);
            INSERT INTO trongate_users (id) VALUES (1);
            INSERT INTO user_llm_keys (trongate_user_id, provider, key_hint, use_for_resume, created_at, updated_at)
            VALUES (1, 'openai', '65sA', 0, 5, 6);
            "#,
        ))
        .await
        .unwrap();
        sqlx::query(r#"INSERT INTO schema_migrations (name, sql, applied_at) VALUES (?1, ?2, 0)"#)
            .bind(first_name)
            .bind(first_sql)
            .execute(&db)
            .await
            .unwrap();

        migrate(&db).await.unwrap();
        let kept: (String, String, i64, i64) =
            sqlx::query_as(r#"SELECT provider, key_hint, use_for_resume, use_for_application FROM user_llm_keys"#)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(kept, ("openai".to_string(), "65sA".to_string(), 0, 1));
        sqlx::query(
            r#"INSERT INTO user_llm_keys (trongate_user_id, provider, created_at, updated_at)
               VALUES (1, 'anthropic', 7, 7)"#,
        )
        .execute(&db)
        .await
        .unwrap();
    }

    /// An in-memory database with the migrations before `upto` applied and recorded.
    async fn migrated_before(upto: usize) -> SqlitePool {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let db = SqlitePoolOptions::new()
            .max_connections(IN_MEMORY_CONNECTIONS)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .unwrap();
        db.execute(sqlx::raw_sql(
            r#"CREATE TABLE schema_migrations (name TEXT PRIMARY KEY, sql TEXT NOT NULL, applied_at INTEGER NOT NULL)"#,
        ))
        .await
        .unwrap();
        for (name, sql) in &MIGRATIONS[..upto] {
            db.execute(sqlx::raw_sql(*sql)).await.unwrap();
            sqlx::query(r#"INSERT INTO schema_migrations (name, sql, applied_at) VALUES (?1, ?2, 0)"#)
                .bind(name)
                .bind(*sql)
                .execute(&db)
                .await
                .unwrap();
        }
        db
    }

    #[tokio::test]
    async fn each_cv_is_kept_once_when_documents_hang_on_matches_through_match_documents() {
        let before = MIGRATIONS
            .iter()
            .position(|(name, _)| *name == "0004_match_documents")
            .unwrap();
        let db = migrated_before(before).await;
        db.execute(sqlx::raw_sql(
            r#"
            INSERT INTO trongate_users (id) VALUES (1);
            INSERT INTO matches (id, trongate_user_id, job_title, job_text, cv_name, cv_text, score, points,
                                 max_points, tag, created_at)
            VALUES (1, 1, 'A', 'post a', 'cv.pdf', 'PHP', 0.8, 8, 10, 'good', 10),
                   (2, 1, 'B', 'post b', 'cv.pdf', 'PHP', 0.7, 7, 10, 'good', 20),
                   (3, 1, 'C', 'post c', 'cv.pdf', 'PHP, Rust', 0.6, 6, 10, 'fair', 30);
            INSERT INTO documents (id, match_id, kind, text, written_at) VALUES (9, 1, 'resume', 'Mette', 40);
            INSERT INTO match_resumes (document_id, name, intro) VALUES (9, 'Mette', 'Backend.');
            "#,
        ))
        .await
        .unwrap();

        migrate(&db).await.unwrap();
        let cvs: Vec<(i64, String)> =
            sqlx::query_as(r#"SELECT id, text FROM documents WHERE kind = 'cv' ORDER BY written_at"#)
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(
            cvs.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>(),
            ["PHP", "PHP, Rust"]
        );
        let links: Vec<(i64, i64)> =
            sqlx::query_as(r#"SELECT match_id, document_id FROM match_documents ORDER BY match_id, document_id"#)
                .fetch_all(&db)
                .await
                .unwrap();
        assert_eq!(links, [(1, 9), (1, cvs[0].0), (2, cvs[0].0), (3, cvs[1].0)]);
        // The résumé keeps its id, and the fields written for it.
        let resume: (String,) = sqlx::query_as(r#"SELECT intro FROM match_resumes WHERE document_id = 9"#)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(resume.0, "Backend.");

        // A match's own documents go with it; a CV stays while another match uses it.
        db.execute(sqlx::raw_sql(r#"DELETE FROM matches WHERE id = 1"#))
            .await
            .unwrap();
        let left: Vec<(i64,)> = sqlx::query_as(r#"SELECT id FROM documents ORDER BY id"#)
            .fetch_all(&db)
            .await
            .unwrap();
        assert_eq!(left, [(cvs[0].0,), (cvs[1].0,)]);
        let fields: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM match_resumes"#)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(fields.0, 0);
        db.execute(sqlx::raw_sql(r#"DELETE FROM matches WHERE id = 2"#))
            .await
            .unwrap();
        let left: (i64,) = sqlx::query_as(r#"SELECT COUNT(*) FROM documents"#)
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(left.0, 1);
    }
}
