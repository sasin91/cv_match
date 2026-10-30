//! Copies one user's cv_match data from the PHP app's MariaDB (a local copy of production, such
//! as Herd's recruiter_prod) into the desktop app's SQLite database. Ids are copied as they are,
//! so every link between rows stays. Only that user's rows are read; other accounts stay behind.
//!
//! Which columns are copied comes from the SQLite tables at run time, so the queries here are
//! built as text rather than checked at compile time; only the SQLite ones that don't depend
//! on the table are.

use std::fmt;

use chrono::{NaiveDate, NaiveDateTime};
use sqlx::mysql::types::MySqlTime;
use sqlx::mysql::{MySqlConnection, MySqlRow};
use sqlx::sqlite::{Sqlite, SqliteArguments, SqliteConnection};
use sqlx::{AssertSqlSafe, Column, Connection, Row, SqlitePool, TypeInfo, ValueRef};

/// Where a table's rows hang on the user, which decides the WHERE that picks out their rows.
#[derive(Clone, Copy)]
enum Owner {
    /// The user's own trongate_users row.
    User,
    /// Rows with the user's trongate_user_id.
    UserId,
    /// Rows with the user's candidate_id.
    Candidate,
    /// Rows of the user's saved matches.
    Match,
}

/// The tables copied, parents before children so every foreign key finds its row. The columns
/// come from the SQLite table, so a column the desktop dropped (the login ones) isn't read.
const TABLES: &[(&str, Owner)] = &[
    ("trongate_users", Owner::User),
    ("candidates", Owner::UserId),
    ("cv_matches", Owner::UserId),
    ("cv_match_items", Owner::Match),
    ("cv_match_integrations", Owner::Match),
    ("cv_match_rejections", Owner::Match),
    ("cv_match_resumes", Owner::Match),
    ("cv_match_resume_entries", Owner::Match),
    ("cv_match_resume_lines", Owner::Match),
    ("candidate_profiles", Owner::Candidate),
    ("candidate_profile_entries", Owner::Candidate),
    ("candidate_profile_lines", Owner::Candidate),
    ("candidate_profile_links", Owner::Candidate),
    ("candidate_resumes", Owner::Candidate),
];

#[derive(Debug)]
pub enum Error {
    MariaDb(sqlx::Error),
    Sqlite(sqlx::Error),
    /// No candidate has that email in the source database.
    NoSuchUser(String),
    /// The desktop database already has a user, so importing would mix two copies.
    AlreadyImported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::MariaDb(e) => write!(f, "MariaDB: {e}"),
            Error::Sqlite(e) => write!(f, "SQLite: {e}"),
            Error::NoSuchUser(email) => write!(f, "no candidate with the email {email} in that database"),
            Error::AlreadyImported => write!(
                f,
                "the desktop database already has your data; delete the file to import again"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// What the import copied: rows per table, in the order they were copied.
#[derive(Debug, Default)]
pub struct Report {
    pub rows: Vec<(&'static str, usize)>,
    /// The source had an LLM key for the user. It is encrypted with the server's secret, so it
    /// isn't copied; the user adds it again in the app.
    pub skipped_llm_key: bool,
}

/// Copies the user with this email from the MariaDB at source_url (mysql://user:pass@host/db)
/// into target, all in one transaction: on any error nothing is written.
pub async fn import(source_url: &str, email: &str, target: &SqlitePool) -> Result<Report, Error> {
    let mut source = MySqlConnection::connect(source_url).await.map_err(Error::MariaDb)?;
    let has_user = sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM trongate_users) AS "has_user: bool""#)
        .fetch_one(target)
        .await
        .map_err(Error::Sqlite)?;
    if has_user {
        return Err(Error::AlreadyImported);
    }
    let (user_id, candidate_id): (i64, i64) =
        sqlx::query_as(r#"SELECT trongate_user_id, id FROM candidates WHERE email = ?"#)
            .bind(email)
            .fetch_optional(&mut source)
            .await
            .map_err(Error::MariaDb)?
            .ok_or_else(|| Error::NoSuchUser(email.to_string()))?;

    let mut tx = target.begin().await.map_err(Error::Sqlite)?;
    let mut report = Report::default();
    for &(table, owner) in TABLES {
        let columns = columns(&mut tx, table).await.map_err(Error::Sqlite)?;
        let rows = select(&mut source, table, &columns, owner, user_id, candidate_id)
            .await
            .map_err(Error::MariaDb)?;
        insert(&mut tx, table, &columns, &rows).await.map_err(Error::Sqlite)?;
        report.rows.push((table, rows.len()));
    }
    tx.commit().await.map_err(Error::Sqlite)?;
    report.skipped_llm_key = sqlx::query(r#"SELECT id FROM user_llm_keys WHERE trongate_user_id = ?"#)
        .bind(user_id)
        .fetch_optional(&mut source)
        .await
        .map_err(Error::MariaDb)?
        .is_some();
    Ok(report)
}

/// The columns of a SQLite table, in their order.
async fn columns(db: &mut SqliteConnection, table: &str) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(r#"SELECT name FROM pragma_table_info(?1) ORDER BY cid"#)
        .bind(table)
        .fetch_all(db)
        .await
}

/// A value as SQLite stores it.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

/// The user's rows of one table, as SQLite values in the columns' order.
async fn select(
    source: &mut MySqlConnection,
    table: &str,
    columns: &[String],
    owner: Owner,
    user_id: i64,
    candidate_id: i64,
) -> sqlx::Result<Vec<Vec<Value>>> {
    let (filter, key) = match owner {
        Owner::User => ("id = ?", user_id),
        Owner::UserId => ("trongate_user_id = ?", user_id),
        Owner::Candidate => ("candidate_id = ?", candidate_id),
        Owner::Match => (
            "cv_match_id IN (SELECT id FROM cv_matches WHERE trongate_user_id = ?)",
            user_id,
        ),
    };
    let quoted: Vec<String> = columns.iter().map(|c| format!("`{c}`")).collect();
    let sql = format!("SELECT {} FROM `{table}` WHERE {filter} ORDER BY 1", quoted.join(", "));
    // Safe: the table is one of TABLES and the columns are SQLite's own names for its columns.
    let rows = sqlx::query(AssertSqlSafe(sql)).bind(key).fetch_all(source).await?;
    rows.iter().map(sqlite_values).collect()
}

/// Inserts rows into a SQLite table, each value in its column.
async fn insert(db: &mut SqliteConnection, table: &str, columns: &[String], rows: &[Vec<Value>]) -> sqlx::Result<()> {
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        columns.join(", "),
        vec!["?"; columns.len()].join(", ")
    );
    for row in rows {
        // Safe: the table is one of TABLES and the columns are SQLite's own names for its columns.
        let mut insert = sqlx::query(AssertSqlSafe(sql.as_str()));
        for value in row {
            insert = bind(insert, value);
        }
        insert.execute(&mut *db).await?;
    }
    Ok(())
}

type SqliteQuery<'q> = sqlx::query::Query<'q, Sqlite, SqliteArguments>;

fn bind<'q>(query: SqliteQuery<'q>, value: &Value) -> SqliteQuery<'q> {
    match value {
        Value::Null => query.bind(None::<i64>),
        Value::Integer(n) => query.bind(*n),
        Value::Real(n) => query.bind(*n),
        Value::Text(text) => query.bind(text.clone()),
    }
}

/// A MariaDB row's values as SQLite stores them.
fn sqlite_values(row: &MySqlRow) -> sqlx::Result<Vec<Value>> {
    (0..row.len()).map(|index| sqlite_value(row, index)).collect()
}

/// A MariaDB value as SQLite stores it. decimal is read as its text ("0.8140") and the REAL
/// column's affinity turns it into a number; a date becomes YYYY-MM-DD, a time HH:MM:SS.
fn sqlite_value(row: &MySqlRow, index: usize) -> sqlx::Result<Value> {
    if row.try_get_raw(index)?.is_null() {
        return Ok(Value::Null);
    }
    let type_name = row.column(index).type_info().name();
    Ok(match type_name {
        "BOOLEAN" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "BIGINT" => Value::Integer(row.try_get(index)?),
        // sqlx reads a year as any other integer, but won't by its type.
        "YEAR" => Value::Integer(row.try_get_unchecked(index)?),
        "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "MEDIUMINT UNSIGNED" | "INT UNSIGNED" | "BIGINT UNSIGNED" => {
            Value::Integer(row.try_get::<u64, _>(index)? as i64)
        }
        "FLOAT" => Value::Real(row.try_get::<f32, _>(index)?.into()),
        "DOUBLE" => Value::Real(row.try_get(index)?),
        "DECIMAL" => Value::Text(row.try_get_unchecked(index)?),
        "DATE" => Value::Text(row.try_get::<NaiveDate, _>(index)?.format("%Y-%m-%d").to_string()),
        "DATETIME" | "TIMESTAMP" => Value::Text(
            row.try_get::<NaiveDateTime, _>(index)?
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ),
        "TIME" => Value::Text(time_text(row.try_get(index)?)),
        // Text, and anything else (binary columns) read as text, as the import always has.
        _ => Value::Text(String::from_utf8_lossy(&row.try_get_unchecked::<Vec<u8>, _>(index)?).into_owned()),
    })
}

/// A TIME as HH:MM:SS, the hours counting past 24 and a minus in front when it is negative.
fn time_text(time: MySqlTime) -> String {
    let sign = if time.sign().is_negative() { "-" } else { "" };
    format!("{sign}{:02}:{:02}:{:02}", time.hours(), time.minutes(), time.seconds())
}
