//! Saved matches in the database (cv_matches with cv_match_items, cv_match_integrations and
//! cv_match_rejections), ported from modules/cv_match/Cv_match_model.php.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;

use super::apply_instructions::{self, ApplyInstructions};
use super::reject_reason::Rejection;
use super::score_cursor;

/// The integration a match's Jobnet joblog status is kept under.
pub const JOBNET_JOBLOG: &str = "jobnet_joblog";

/// The Jobnet statuses that mean the candidate applied (Joblog.php's APPLIED_STATUSES).
pub const APPLIED_STATUSES: [&str; 2] = ["Soegt", "Samtale"];

/// The lists of saved matches by ?list=, each with its tab label and what the page says when it
/// has none: the shortlist (top matches not applied for yet), all of them, applied for but not
/// on Jobnet yet, on Jobnet, and rejected.
pub const SHORTLIST: &str = "shortlist";
pub const LISTS: [(&str, &str, &str); 5] = [
    (
        SHORTLIST,
        "Shortlist",
        "No top matches left to apply for. All has the rest.",
    ),
    ("all", "All", "No saved matches yet."),
    (
        "not_joblogged",
        "Applied, not on Jobnet yet",
        "Nothing to log: every match marked Søgt or Samtale in its Jobnet joblog panel is on Jobnet.",
    ),
    (
        "joblogged",
        "On Jobnet",
        "Nothing on Jobnet yet: tick \"entered on Jobnet\" in a match's joblog panel once you have logged it there.",
    ),
    (
        "rejected",
        "Rejected",
        "Nothing rejected. Reject a match you are not interested in to keep it out of the other lists.",
    ),
];

/// One scored item of a saved match: the "why" behind its score.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedItem {
    pub criterion: String,
    pub text: String,
    pub verdict: String,
    pub decided_by: String,
    pub reason: String,
    pub evidence: String,
}

/// A saved match's state in an outside service (Jobnet's joblog).
#[derive(Debug, Clone, PartialEq)]
pub struct Integration {
    pub status: String,
    pub external_id: Option<String>,
    pub applied_at: Option<i64>,
    pub logged_at: Option<i64>,
    pub updated_at: i64,
}

/// A saved match with everything the page shows of it.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedMatch {
    pub id: i64,
    pub trongate_user_id: Option<i64>,
    pub job_title: String,
    pub company: String,
    pub job_url: Option<String>,
    pub job_text: String,
    pub cv_name: String,
    pub cv_text: String,
    pub score: f64,
    pub points: i64,
    pub max_points: i64,
    pub tag: String,
    pub application_text: Option<String>,
    pub application_written_at: Option<i64>,
    pub resume_text: Option<String>,
    pub resume_written_at: Option<i64>,
    pub rejected_at: Option<i64>,
    pub created_at: i64,
    /// Its tailored résumé was written as fields (cv_match_resumes).
    pub resume_structured: bool,
    pub items: Vec<SavedItem>,
    pub jobnet_joblog: Option<Integration>,
    pub rejections: Vec<Rejection>,
    /// How the post asks to be applied to (apply_instructions::read of job_text).
    pub apply_instructions: Option<ApplyInstructions>,
}

/// A saved match as a list shows it (and the Jobnet extension lists it, as JSON).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListedMatch {
    pub id: i64,
    pub job_title: String,
    pub company: String,
    pub job_url: Option<String>,
    pub cv_name: String,
    pub score: f64,
    pub points: i64,
    pub max_points: i64,
    pub tag: String,
    pub has_application: bool,
    pub has_resume: bool,
    pub created_at: i64,
    /// The first location the post requires that the match misses.
    pub location_missed: Option<String>,
    pub jobnet_status: Option<String>,
    pub jobnet_logged_at: Option<i64>,
    #[serde(skip)]
    pub rejected_at: Option<i64>,
    #[serde(skip)]
    pub rejections: Vec<Rejection>,
}

/// A batch of a list, and the cursor of the batch after it (None at the end).
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    pub rows: Vec<ListedMatch>,
    pub next: Option<String>,
}

const LISTED_COLUMNS: &str = "m.id, m.job_title, m.company, m.job_url, m.cv_name, m.score, m.points, m.max_points, m.tag,
    m.application_text IS NOT NULL AS has_application,
    m.resume_text IS NOT NULL AS has_resume, m.created_at,
    (SELECT i.text FROM cv_match_items i WHERE i.cv_match_id = m.id AND i.criterion = 'location' AND i.verdict = 'missing' ORDER BY i.sort_order LIMIT 1) AS location_missed,
    j.status AS jobnet_status, j.logged_at AS jobnet_logged_at,
    m.rejected_at";

fn listed(row: &Row) -> rusqlite::Result<ListedMatch> {
    Ok(ListedMatch {
        id: row.get("id")?,
        job_title: row.get("job_title")?,
        company: row.get("company")?,
        job_url: row.get("job_url")?,
        cv_name: row.get("cv_name")?,
        score: row.get("score")?,
        points: row.get("points")?,
        max_points: row.get("max_points")?,
        tag: row.get("tag")?,
        has_application: row.get("has_application")?,
        has_resume: row.get("has_resume")?,
        created_at: row.get("created_at")?,
        location_missed: row.get("location_missed")?,
        jobnet_status: row.get("jobnet_status")?,
        jobnet_logged_at: row.get("jobnet_logged_at")?,
        rejected_at: row.get("rejected_at")?,
        rejections: Vec::new(),
    })
}

/// The user's newest matches not rejected, for the extension's history.
pub fn recent(db: &Connection, user_id: i64, limit: usize) -> rusqlite::Result<Vec<ListedMatch>> {
    let sql = format!(
        "SELECT {LISTED_COLUMNS}
         FROM cv_matches m
         LEFT JOIN cv_match_integrations j ON j.cv_match_id = m.id AND j.integration = '{JOBNET_JOBLOG}'
         WHERE m.trongate_user_id IS ?1 AND m.rejected_at IS NULL
         ORDER BY m.created_at DESC, m.id DESC LIMIT ?2"
    );
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map(params![user_id, limit.max(1) as i64], listed)?;
    rows.collect()
}

/// A batch of the user's matches in list `list` (LISTS), best first (highest score, then
/// newest), after cursor `after` (score_cursor) when it is one.
pub fn best_first(db: &Connection, user_id: i64, limit: usize, after: &str, list: &str) -> rusqlite::Result<Batch> {
    let place = score_cursor::place(after);
    let after_place = if place.is_some() {
        " AND (m.score, m.created_at, m.id) < (?3, ?4, ?5)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT {LISTED_COLUMNS}
         FROM cv_matches m
         LEFT JOIN cv_match_integrations j ON j.cv_match_id = m.id AND j.integration = '{JOBNET_JOBLOG}'
         WHERE m.trongate_user_id IS ?1 AND {}{after_place}
         ORDER BY m.score DESC, m.created_at DESC, m.id DESC
         LIMIT ?2",
        list_condition(list)
    );
    let mut statement = db.prepare(&sql)?;
    let fetch = limit as i64 + 1;
    let mut rows: Vec<ListedMatch> = match place {
        Some(place) => statement
            .query_map(params![user_id, fetch, place.score, place.at, place.id], listed)?
            .collect::<rusqlite::Result<_>>()?,
        None => statement
            .query_map(params![user_id, fetch], listed)?
            .collect::<rusqlite::Result<_>>()?,
    };
    let mut next = None;
    if rows.len() > limit {
        rows.truncate(limit);
        let last = rows.last().expect("limit rows");
        next = Some(score_cursor::after(Some(last.score), last.created_at, last.id));
    }
    let rejected: Vec<i64> = rows
        .iter()
        .filter(|row| row.rejected_at.is_some())
        .map(|row| row.id)
        .collect();
    let mut by_match = rejections(db, &rejected)?;
    for row in &mut rows {
        row.rejections = by_match.remove(&row.id).unwrap_or_default();
    }
    Ok(Batch { rows, next })
}

/// The condition for a list of matches by its name: 'shortlist' for the top and good ones not
/// applied for yet, 'not_joblogged' for the ones applied for but not yet entered in Jobnet's
/// joblog, 'joblogged' for the ones entered there, 'rejected' for the ones the user rejected;
/// every other list leaves the rejected ones out.
fn list_condition(list: &str) -> String {
    let applied = format!("('{}')", APPLIED_STATUSES.join("', '"));
    match list {
        SHORTLIST => format!(
            "m.rejected_at IS NULL AND m.tag IN ('top', 'good') AND (j.status IS NULL OR j.status NOT IN {applied}) AND j.logged_at IS NULL"
        ),
        "not_joblogged" => format!("m.rejected_at IS NULL AND j.status IN {applied} AND j.logged_at IS NULL"),
        "joblogged" => "m.rejected_at IS NULL AND j.logged_at IS NOT NULL".to_string(),
        "rejected" => "m.rejected_at IS NOT NULL".to_string(),
        _ => "m.rejected_at IS NULL".to_string(),
    }
}

/// Why matches `ids` were rejected, by match id, in the order ticked.
fn rejections(db: &Connection, ids: &[i64]) -> rusqlite::Result<HashMap<i64, Vec<Rejection>>> {
    let mut by_match: HashMap<i64, Vec<Rejection>> = HashMap::new();
    if ids.is_empty() {
        return Ok(by_match);
    }
    let marks = vec!["?"; ids.len()].join(", ");
    let mut statement = db.prepare(&format!(
        "SELECT cv_match_id, reason, note FROM cv_match_rejections WHERE cv_match_id IN ({marks}) ORDER BY id"
    ))?;
    let rows = statement.query_map(rusqlite::params_from_iter(ids), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            Rejection {
                reason: row.get(1)?,
                note: row.get(2)?,
            },
        ))
    })?;
    for row in rows {
        let (id, rejection) = row?;
        by_match.entry(id).or_default().push(rejection);
    }
    Ok(by_match)
}

/// The id of the user's newest match of this CV with the same job post text, or None. By text,
/// not link: a page can change, and a post read wrongly before is matched again once it reads
/// right.
pub fn existing(db: &Connection, user_id: i64, cv_text: &str, job_text: &str) -> rusqlite::Result<Option<i64>> {
    if cv_text.is_empty() || job_text.is_empty() {
        return Ok(None);
    }
    db.query_row(
        "SELECT id FROM cv_matches WHERE trongate_user_id IS ?1 AND cv_text = ?2 AND job_text = ?3
         ORDER BY created_at DESC, id DESC LIMIT 1",
        params![user_id, cv_text, job_text],
        |row| row.get(0),
    )
    .optional()
}

/// One of the user's saved matches with its items, joblog status and rejections, or None.
pub fn find(db: &Connection, id: i64, user_id: i64) -> rusqlite::Result<Option<SavedMatch>> {
    let found = db
        .query_row(
            "SELECT cv_matches.*,
                    EXISTS (SELECT 1 FROM cv_match_resumes r WHERE r.cv_match_id = cv_matches.id) AS resume_structured
             FROM cv_matches WHERE id = ?1 AND trongate_user_id IS ?2",
            params![id, user_id],
            |row| {
                Ok(SavedMatch {
                    id: row.get("id")?,
                    trongate_user_id: row.get("trongate_user_id")?,
                    job_title: row.get("job_title")?,
                    company: row.get("company")?,
                    job_url: row.get("job_url")?,
                    job_text: row.get("job_text")?,
                    cv_name: row.get("cv_name")?,
                    cv_text: row.get("cv_text")?,
                    score: row.get("score")?,
                    points: row.get("points")?,
                    max_points: row.get("max_points")?,
                    tag: row.get("tag")?,
                    application_text: row.get("application_text")?,
                    application_written_at: row.get("application_written_at")?,
                    resume_text: row.get("resume_text")?,
                    resume_written_at: row.get("resume_written_at")?,
                    rejected_at: row.get("rejected_at")?,
                    created_at: row.get("created_at")?,
                    resume_structured: row.get("resume_structured")?,
                    items: Vec::new(),
                    jobnet_joblog: None,
                    rejections: Vec::new(),
                    apply_instructions: None,
                })
            },
        )
        .optional()?;
    let Some(mut found) = found else {
        return Ok(None);
    };
    let mut statement = db.prepare(
        "SELECT criterion, text, verdict, decided_by, reason, evidence
         FROM cv_match_items WHERE cv_match_id = ?1 ORDER BY sort_order, id",
    )?;
    found.items = statement
        .query_map([id], |row| {
            Ok(SavedItem {
                criterion: row.get(0)?,
                text: row.get(1)?,
                verdict: row.get(2)?,
                decided_by: row.get(3)?,
                reason: row.get(4)?,
                evidence: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    found.jobnet_joblog = integration(db, id, JOBNET_JOBLOG)?;
    found.rejections = rejections(db, &[id])?.remove(&id).unwrap_or_default();
    found.apply_instructions = apply_instructions::read(&found.job_text);
    Ok(Some(found))
}

/// A match's state in one integration, or None.
pub fn integration(db: &Connection, id: i64, integration: &str) -> rusqlite::Result<Option<Integration>> {
    db.query_row(
        "SELECT status, external_id, applied_at, logged_at, updated_at FROM cv_match_integrations
         WHERE cv_match_id = ?1 AND integration = ?2",
        params![id, integration],
        |row| {
            Ok(Integration {
                status: row.get(0)?,
                external_id: row.get(1)?,
                applied_at: row.get(2)?,
                logged_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
    .optional()
}

/// Saves a match's status in an outside service. `logged` says whether the candidate has
/// entered it there; the first time keeps its date. The first applied status
/// (APPLIED_STATUSES) keeps its date too (applied_at), through Søgt -> Samtale; any other status
/// clears it.
pub fn save_integration(
    db: &Connection,
    id: i64,
    integration: &str,
    status: &str,
    logged: bool,
    now: i64,
) -> rusqlite::Result<Option<Integration>> {
    let applied = APPLIED_STATUSES.contains(&status);
    db.execute(
        "INSERT INTO cv_match_integrations (cv_match_id, integration, status, applied_at, logged_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT (cv_match_id, integration) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at,
             applied_at = CASE WHEN ?7 THEN COALESCE(applied_at, excluded.applied_at) END,
             logged_at = CASE WHEN ?8 THEN COALESCE(logged_at, excluded.logged_at) END",
        params![
            id,
            integration,
            status,
            applied.then_some(now),
            logged.then_some(now),
            now,
            applied,
            logged
        ],
    )?;
    self::integration(db, id, integration)
}

/// Rejects one of the user's matches (not interested), with why; false when it isn't theirs.
pub fn reject(db: &mut Connection, id: i64, user_id: i64, rows: &[Rejection], now: i64) -> rusqlite::Result<bool> {
    let tx = db.transaction()?;
    if !owns(&tx, id, user_id)? {
        return Ok(false);
    }
    tx.execute("UPDATE cv_matches SET rejected_at = ?1 WHERE id = ?2", params![now, id])?;
    tx.execute("DELETE FROM cv_match_rejections WHERE cv_match_id = ?1", [id])?;
    for row in rows {
        tx.execute(
            "INSERT INTO cv_match_rejections (cv_match_id, reason, note, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, row.reason, row.note, now],
        )?;
    }
    tx.commit()?;
    Ok(true)
}

/// Brings back one of the user's rejected matches; false when it isn't theirs.
pub fn bring_back(db: &mut Connection, id: i64, user_id: i64) -> rusqlite::Result<bool> {
    let tx = db.transaction()?;
    if !owns(&tx, id, user_id)? {
        return Ok(false);
    }
    tx.execute("UPDATE cv_matches SET rejected_at = NULL WHERE id = ?1", [id])?;
    tx.execute("DELETE FROM cv_match_rejections WHERE cv_match_id = ?1", [id])?;
    tx.commit()?;
    Ok(true)
}

fn owns(db: &Connection, id: i64, user_id: i64) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS (SELECT 1 FROM cv_matches WHERE id = ?1 AND trongate_user_id IS ?2)",
        params![id, user_id],
        |row| row.get(0),
    )
}
