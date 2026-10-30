//! The candidate's profile as text, the CV the page matches with when there is one, ported from
//! modules/profile (Profile_fields::text, Profile_model's queries) and account's profile links.

use rusqlite::{Connection, OptionalExtension, params};

use chrono::NaiveDate;

/// The entry sections in the order the text lists them, with their headings.
pub const SECTIONS: [(&str, &str); 3] = [
    ("experience", "Experience"),
    ("project", "Projects"),
    ("education", "Education"),
];
/// The lists of single lines in the order the text lists them, with their headings.
pub const LISTS: [(&str, &str); 3] = [
    ("skill", "Skills"),
    ("language", "Languages"),
    ("certificate", "Certificates"),
];

struct Entry {
    section: String,
    title: String,
    organisation: String,
    location: String,
    url: String,
    started_on: Option<String>,
    ended_on: Option<String>,
    summary: String,
    note: String,
    bullets: Vec<String>,
}

/// The user's profile as text, or None when they have none.
pub fn text_for_user(db: &Connection, user_id: i64) -> rusqlite::Result<Option<String>> {
    let Some((candidate_id, name, email, phone)) = db
        .query_row(
            "SELECT id, name, email, phone FROM candidates WHERE trongate_user_id = ?1",
            [user_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let Some((title, location, about)) = db
        .query_row(
            "SELECT title, location, about FROM candidate_profiles WHERE candidate_id = ?1",
            [candidate_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let entries = entries(db, candidate_id)?;
    let links = profile_links(db, candidate_id)?;

    let phone = phone.unwrap_or_default();
    let contact = join_present(&[&location, &phone, &email], " · ");
    let link_line = links
        .iter()
        .map(|(label, url)| format!("{label}: {url}"))
        .collect::<Vec<_>>()
        .join(" · ");
    let mut blocks = vec![join_present(&[&name, &title, &contact, &link_line], "\n")];
    let about = about.trim();
    if !about.is_empty() {
        blocks.push(about.to_string());
    }
    for (section, heading) in SECTIONS {
        let texts: Vec<String> = entries
            .iter()
            .filter(|e| e.section == section)
            .map(entry_text)
            .collect();
        if !texts.is_empty() {
            blocks.push(format!("{heading}\n\n{}", texts.join("\n\n")));
        }
    }
    for (kind, heading) in LISTS {
        let lines = lines(db, candidate_id, kind)?;
        if !lines.is_empty() {
            blocks.push(format!(
                "{heading}\n{}",
                lines
                    .iter()
                    .map(|line| format!("- {line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    Ok(Some(blocks.join("\n\n")))
}

/// The candidate's profile links, (label, url) in their order.
pub fn profile_links(db: &Connection, candidate_id: i64) -> rusqlite::Result<Vec<(String, String)>> {
    let mut statement =
        db.prepare("SELECT label, url FROM candidate_profile_links WHERE candidate_id = ?1 ORDER BY position, id")?;
    let rows = statement.query_map([candidate_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

/// The parts that aren't empty, joined (PHP's implode of array_filter).
fn join_present(parts: &[&str], glue: &str) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty() && **part != "0")
        .copied()
        .collect::<Vec<_>>()
        .join(glue)
}

/// The entries, each with its bullets, by section, newest first: ongoing ones on top, then by
/// when they ended and started; the CV's order breaks ties.
fn entries(db: &Connection, candidate_id: i64) -> rusqlite::Result<Vec<Entry>> {
    let mut statement = db.prepare(
        "SELECT id, section, title, organisation, location, url, started_on, ended_on, summary, note
         FROM candidate_profile_entries WHERE candidate_id = ?1
         ORDER BY CASE section WHEN 'experience' THEN 1 WHEN 'project' THEN 2 WHEN 'education' THEN 3 ELSE 0 END,
                  ended_on IS NULL DESC, ended_on DESC, started_on DESC, sort_order, id",
    )?;
    let mut bullets = db.prepare(
        "SELECT text FROM candidate_profile_lines WHERE candidate_id = ?1 AND entry_id = ?2 AND kind = 'bullet' ORDER BY sort_order, id",
    )?;
    let rows: Vec<(i64, Entry)> = statement
        .query_map([candidate_id], |row| {
            Ok((
                row.get(0)?,
                Entry {
                    section: row.get(1)?,
                    title: row.get(2)?,
                    organisation: row.get(3)?,
                    location: row.get(4)?,
                    url: row.get(5)?,
                    started_on: row.get(6)?,
                    ended_on: row.get(7)?,
                    summary: row.get(8)?,
                    note: row.get(9)?,
                    bullets: Vec::new(),
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut entries = Vec::with_capacity(rows.len());
    for (id, mut entry) in rows {
        entry.bullets = bullets
            .query_map(params![candidate_id, id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        entries.push(entry);
    }
    Ok(entries)
}

fn lines(db: &Connection, candidate_id: i64, kind: &str) -> rusqlite::Result<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT text FROM candidate_profile_lines WHERE candidate_id = ?1 AND entry_id IS NULL AND kind = ?2 ORDER BY sort_order, id",
    )?;
    let rows = statement.query_map(params![candidate_id, kind], |row| row.get(0))?;
    rows.collect()
}

fn entry_text(entry: &Entry) -> String {
    let heading = join_present(&[&entry.title, &entry.organisation], " · ");
    let months = months(entry.started_on.as_deref(), entry.ended_on.as_deref());
    let meta = join_present(&[&months, &entry.location, &entry.url], " — ");
    let mut text = join_present(&[&heading, &meta, &entry.summary], "\n");
    if !entry.bullets.is_empty() {
        text.push('\n');
        text.push_str(
            &entry
                .bullets
                .iter()
                .map(|bullet| format!("- {bullet}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if !entry.note.is_empty() {
        text.push('\n');
        text.push_str(&entry.note);
    }
    text
}

/// "September 2024 – February 2026", "January 2023 – now", or ''.
pub fn months(started_on: Option<&str>, ended_on: Option<&str>) -> String {
    if started_on.is_none() && ended_on.is_none() {
        return String::new();
    }
    let month = |date: Option<&str>| {
        date.and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .map(|date| date.format("%B %Y").to_string())
            .unwrap_or_default()
    };
    let ended = if ended_on.is_some() {
        month(ended_on)
    } else {
        "now".to_string()
    };
    format!("{} – {ended}", month(started_on))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn the_profile_reads_as_a_cv() {
        let db = db::open_in_memory().unwrap();
        db.execute_batch(
            "INSERT INTO trongate_users (id) VALUES (3);
             INSERT INTO candidates (id, trongate_user_id, name, email, phone, created_at, updated_at)
             VALUES (11, 3, 'Jonas', 'me@example.com', '12345678', 0, 0);
             INSERT INTO candidate_profiles (candidate_id, title, location, about, created_at, updated_at)
             VALUES (11, 'Software developer', 'Slagelse', ' I build things. ', 0, 0);
             INSERT INTO candidate_profile_entries (id, candidate_id, section, title, organisation, started_on, ended_on, sort_order, created_at, updated_at)
             VALUES (1, 11, 'experience', 'Developer', 'Old Firm', '2017-01-01', '2020-03-01', 0, 0, 0),
                    (2, 11, 'experience', 'Lead', 'New Firm', '2020-04-01', NULL, 1, 0, 0),
                    (3, 11, 'education', 'Datamatiker', 'Zealand', NULL, '2016-06-01', 0, 0, 0);
             INSERT INTO candidate_profile_lines (candidate_id, entry_id, kind, text, sort_order)
             VALUES (11, 2, 'bullet', 'Built the platform', 0), (11, NULL, 'skill', 'PHP', 0), (11, NULL, 'skill', 'Rust', 1);
             INSERT INTO candidate_profile_links (candidate_id, label, url, position, created_at, updated_at)
             VALUES (11, 'GitHub', 'https://github.com/sasin91', 0, 0, 0);",
        )
        .unwrap();
        assert_eq!(
            text_for_user(&db, 3).unwrap().unwrap(),
            "Jonas\nSoftware developer\nSlagelse · 12345678 · me@example.com\nGitHub: https://github.com/sasin91\n\n\
             I build things.\n\n\
             Experience\n\nLead · New Firm\nApril 2020 – now\n- Built the platform\n\n\
             Developer · Old Firm\nJanuary 2017 – March 2020\n\n\
             Education\n\nDatamatiker · Zealand\n – June 2016\n\n\
             Skills\n- PHP\n- Rust"
        );
        assert_eq!(text_for_user(&db, 4).unwrap(), None);
    }
}
