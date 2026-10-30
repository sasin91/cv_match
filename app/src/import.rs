//! Copies one user's cv_match data from the PHP app's MariaDB (a local copy of production, such
//! as Herd's recruiter_prod) into the desktop app's SQLite database. Ids are copied as they are,
//! so every link between rows stays. Only that user's rows are read; other accounts stay behind.
//!
//! Which columns are copied comes from the SQLite tables at run time, so the queries here are
//! built as text rather than checked at compile time; only the SQLite ones that don't depend
//! on the table are.
//!
//! The desktop's tables drop the cv_match prefix MariaDB's have (cv_matches is matches,
//! cv_match_id is match_id), so each table is read under its MariaDB name. The documents are the
//! one place the tables differ beyond names: MariaDB keeps a match's application and résumé in
//! cv_matches (application_text, resume_text) and the résumé's fields on the match, the desktop
//! keeps them as versions in documents with the fields on the résumé version. Each written one
//! becomes its first version.

use std::collections::HashMap;
use std::fmt;

use chrono::{NaiveDate, NaiveDateTime};
use sqlx::mysql::types::MySqlTime;
use sqlx::mysql::{MySqlConnection, MySqlRow};
use sqlx::sqlite::{Sqlite, SqliteArguments, SqliteConnection};
use sqlx::{AssertSqlSafe, Column, Connection, Row, SqlitePool, TypeInfo, ValueRef};

use crate::matches::document_box::{APPLICATION, RESUME};

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
    /// The documents in the user's cv_matches rows, one version each (import_documents).
    Documents,
    /// Rows of a résumé's fields: MariaDB's cv_match_id, here the résumé's document_id.
    Resume,
}

/// The column a match's rows hang on here, and the one in MariaDB it is read from.
const MATCH_ID: &str = "match_id";
const MARIADB_MATCH_ID: &str = "cv_match_id";

/// The column the résumé tables hang on here; MariaDB's is the match's (MARIADB_MATCH_ID).
const DOCUMENT_ID: &str = "document_id";

/// The tables copied, parents before children so every foreign key finds its row: the desktop's
/// name, then MariaDB's. The columns come from the SQLite table, so a column the desktop dropped
/// (the login ones) isn't read.
const TABLES: &[(&str, &str, Owner)] = &[
    ("trongate_users", "trongate_users", Owner::User),
    ("candidates", "candidates", Owner::UserId),
    ("matches", "cv_matches", Owner::UserId),
    ("documents", "cv_matches", Owner::Documents),
    ("match_items", "cv_match_items", Owner::Match),
    ("match_integrations", "cv_match_integrations", Owner::Match),
    ("match_rejections", "cv_match_rejections", Owner::Match),
    ("match_resumes", "cv_match_resumes", Owner::Resume),
    ("match_resume_entries", "cv_match_resume_entries", Owner::Resume),
    ("match_resume_lines", "cv_match_resume_lines", Owner::Resume),
    ("candidate_profiles", "candidate_profiles", Owner::Candidate),
    (
        "candidate_profile_entries",
        "candidate_profile_entries",
        Owner::Candidate,
    ),
    ("candidate_profile_lines", "candidate_profile_lines", Owner::Candidate),
    ("candidate_profile_links", "candidate_profile_links", Owner::Candidate),
    ("candidate_resumes", "candidate_resumes", Owner::Candidate),
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
    // Each match's résumé document, by match id, for the résumé tables after it.
    let mut resumes = HashMap::new();
    for &(table, source_table, owner) in TABLES {
        if let Owner::Documents = owner {
            let documents = documents(&mut source, user_id).await.map_err(Error::MariaDb)?;
            resumes = insert_documents(&mut tx, &documents).await.map_err(Error::Sqlite)?;
            report.rows.push((table, documents.len()));
            continue;
        }
        let columns = columns(&mut tx, table).await.map_err(Error::Sqlite)?;
        let mut rows = select(&mut source, source_table, &columns, owner, user_id, candidate_id)
            .await
            .map_err(Error::MariaDb)?;
        if let Owner::Resume = owner {
            to_resume_documents(&columns, &mut rows, &resumes);
        }
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

/// A document in one of the user's MariaDB cv_matches rows.
#[derive(Debug)]
struct ImportedDocument {
    match_id: i64,
    kind: &'static str,
    text: String,
    written_at: i64,
}

/// A MariaDB cv_matches row's documents.
#[derive(sqlx::FromRow)]
struct MatchDocuments {
    id: i64,
    application_text: Option<String>,
    application_written_at: Option<i64>,
    resume_text: Option<String>,
    resume_written_at: Option<i64>,
    /// 1 when the résumé has fields (cv_match_resumes), else 0.
    resume_structured: i64,
    created_at: i64,
}

/// The user's applications and résumés, from their cv_matches rows. A résumé with fields but
/// no text gets an empty text, so its fields keep a document to hang on.
async fn documents(source: &mut MySqlConnection, user_id: i64) -> sqlx::Result<Vec<ImportedDocument>> {
    let rows: Vec<MatchDocuments> = sqlx::query_as(
        r#"
        SELECT m.id, m.application_text, m.application_written_at, m.resume_text, m.resume_written_at,
               EXISTS (SELECT 1 FROM cv_match_resumes r WHERE r.cv_match_id = m.id) AS resume_structured,
               m.created_at
        FROM cv_matches m
        WHERE m.trongate_user_id = ?
        ORDER BY m.id
        "#,
    )
    .bind(user_id)
    .fetch_all(source)
    .await?;
    let mut documents = Vec::new();
    for row in rows {
        if let Some(text) = row.application_text {
            documents.push(ImportedDocument {
                match_id: row.id,
                kind: APPLICATION,
                text,
                written_at: row.application_written_at.unwrap_or(row.created_at),
            });
        }
        if row.resume_text.is_some() || row.resume_structured != 0 {
            documents.push(ImportedDocument {
                match_id: row.id,
                kind: RESUME,
                text: row.resume_text.unwrap_or_default(),
                written_at: row.resume_written_at.unwrap_or(row.created_at),
            });
        }
    }
    Ok(documents)
}

/// Inserts the documents; answers each match's résumé document id, by match id.
async fn insert_documents(
    db: &mut SqliteConnection,
    documents: &[ImportedDocument],
) -> sqlx::Result<HashMap<i64, i64>> {
    let mut resumes = HashMap::new();
    for document in documents {
        let id = sqlx::query!(
            r#"
            INSERT INTO documents (match_id, kind, text, written_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            document.match_id,
            document.kind,
            document.text,
            document.written_at
        )
        .execute(&mut *db)
        .await?
        .last_insert_rowid();
        if document.kind == RESUME {
            resumes.insert(document.match_id, id);
        }
    }
    Ok(resumes)
}

/// Puts each résumé row's document in place of the match MariaDB hangs it on.
fn to_resume_documents(columns: &[String], rows: &mut [Vec<Value>], resumes: &HashMap<i64, i64>) {
    let Some(key) = columns.iter().position(|column| column == DOCUMENT_ID) else {
        return;
    };
    for row in rows {
        if let Value::Integer(match_id) = row[key] {
            // Every match with résumé rows got a résumé document (documents()).
            row[key] = resumes.get(&match_id).map_or(Value::Null, |id| Value::Integer(*id));
        }
    }
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

/// The user's rows of one MariaDB table, as SQLite values in the SQLite columns' order.
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
        Owner::Match | Owner::Resume => (
            "cv_match_id IN (SELECT id FROM cv_matches WHERE trongate_user_id = ?)",
            user_id,
        ),
        Owner::Documents => unreachable!("documents are read by documents()"),
    };
    let quoted: Vec<String> = columns.iter().map(|c| source_column(c, owner)).collect();
    let sql = format!("SELECT {} FROM `{table}` WHERE {filter} ORDER BY 1", quoted.join(", "));
    // Safe: the table is one of TABLES and the columns are SQLite's own names for its columns
    // (or MariaDB's for them, source_column).
    let rows = sqlx::query(AssertSqlSafe(sql)).bind(key).fetch_all(source).await?;
    rows.iter().map(sqlite_values).collect()
}

/// The MariaDB column a SQLite column is read from, quoted: its own name, but match_id is
/// cv_match_id there, and a résumé row's document_id comes from its cv_match_id too
/// (to_resume_documents swaps the id).
fn source_column(column: &str, owner: Owner) -> String {
    match owner {
        _ if column == MATCH_ID => format!("`{MARIADB_MATCH_ID}`"),
        Owner::Resume if column == DOCUMENT_ID => format!("`{MARIADB_MATCH_ID}`"),
        _ => format!("`{column}`"),
    }
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
