//! Copies one user's cv_match data from the PHP app's MariaDB (a local copy of production, such
//! as Herd's recruiter_prod) into the desktop app's SQLite database. Ids are copied as they are,
//! so every link between rows stays. Only that user's rows are read; other accounts stay behind.

use std::fmt;

use mysql::prelude::Queryable;
use rusqlite::Connection;
use rusqlite::types::Value;

use crate::db;

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
    MariaDb(mysql::Error),
    Sqlite(rusqlite::Error),
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

impl From<mysql::Error> for Error {
    fn from(e: mysql::Error) -> Self {
        Error::MariaDb(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

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
pub fn import(source_url: &str, email: &str, target: &mut Connection) -> Result<Report, Error> {
    let mut source = mysql::Conn::new(mysql::Opts::from_url(source_url).map_err(mysql::Error::from)?)?;
    let has_user: bool = target.query_row("SELECT EXISTS (SELECT 1 FROM trongate_users)", [], |row| row.get(0))?;
    if has_user {
        return Err(Error::AlreadyImported);
    }
    let (user_id, candidate_id): (i64, i64) = source
        .exec_first("SELECT trongate_user_id, id FROM candidates WHERE email = ?", (email,))?
        .ok_or_else(|| Error::NoSuchUser(email.to_string()))?;

    let tx = target.transaction()?;
    let mut report = Report::default();
    for &(table, owner) in TABLES {
        let columns = db::columns(&tx, table)?;
        let rows = select(&mut source, table, &columns, owner, user_id, candidate_id)?;
        let insert = format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            columns.join(", "),
            vec!["?"; columns.len()].join(", ")
        );
        let mut statement = tx.prepare(&insert)?;
        for row in &rows {
            statement.execute(rusqlite::params_from_iter(row))?;
        }
        report.rows.push((table, rows.len()));
    }
    tx.commit()?;
    report.skipped_llm_key = source
        .exec_first::<i64, _, _>("SELECT id FROM user_llm_keys WHERE trongate_user_id = ?", (user_id,))?
        .is_some();
    Ok(report)
}

/// The user's rows of one table, as SQLite values in the columns' order.
fn select(
    source: &mut mysql::Conn,
    table: &str,
    columns: &[String],
    owner: Owner,
    user_id: i64,
    candidate_id: i64,
) -> Result<Vec<Vec<Value>>, Error> {
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
    let rows: Vec<mysql::Row> = source.exec(sql, (key,))?;
    Ok(rows
        .into_iter()
        .map(|row| row.unwrap().into_iter().map(sqlite_value).collect())
        .collect())
}

/// A MariaDB value as SQLite stores it. decimal arrives as text ("0.8140") and the REAL
/// column's affinity turns it into a number; a date becomes YYYY-MM-DD.
fn sqlite_value(value: mysql::Value) -> Value {
    match value {
        mysql::Value::NULL => Value::Null,
        mysql::Value::Bytes(bytes) => Value::Text(String::from_utf8_lossy(&bytes).into_owned()),
        mysql::Value::Int(n) => Value::Integer(n),
        mysql::Value::UInt(n) => Value::Integer(n as i64),
        mysql::Value::Float(n) => Value::Real(n.into()),
        mysql::Value::Double(n) => Value::Real(n),
        mysql::Value::Date(year, month, day, 0, 0, 0, 0) => Value::Text(format!("{year:04}-{month:02}-{day:02}")),
        mysql::Value::Date(year, month, day, hour, minute, second, _) => Value::Text(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
        )),
        mysql::Value::Time(negative, days, hours, minutes, seconds, _) => {
            let sign = if negative { "-" } else { "" };
            Value::Text(format!(
                "{sign}{:02}:{minutes:02}:{seconds:02}",
                days * 24 + u32::from(hours)
            ))
        }
    }
}
