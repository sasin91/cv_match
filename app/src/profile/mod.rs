//! The candidate's profile as text, the CV the page matches with when there is one, ported from
//! modules/profile (Profile_fields::text, Profile_model's queries) and account's profile links.

pub mod profile_fields;
pub mod profile_fill;
pub mod profile_model;

use chrono::NaiveDate;
use sqlx::SqlitePool;

use crate::matches::free_reader::{self, YearMonth};

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

/// The profile's one row: profile.id is always this (its CHECK, migration 0011_one_user_schema).
pub const PROFILE_ID: i64 = 1;

/// The profile's top: who the candidate is and how to reach them, as the profile page has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contact {
    pub name: String,
    pub email: String,
    /// "" when the profile has none.
    pub phone: String,
    pub title: String,
    pub location: String,
    pub about: String,
}

/// The profile's top, or None when the user has no profile.
pub async fn contact(db: &SqlitePool) -> sqlx::Result<Option<Contact>> {
    let row = sqlx::query!(r#"SELECT name, email, phone, title, location, about FROM profile"#)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|row| Contact {
        name: row.name,
        email: row.email,
        phone: row.phone.unwrap_or_default(),
        title: row.title,
        location: row.location,
        about: row.about,
    }))
}

/// The user's profile as text, or None when they have none.
pub async fn text_for_user(db: &SqlitePool) -> sqlx::Result<Option<String>> {
    text_for_user_at(db, YearMonth::of(chrono::Utc::now().timestamp())).await
}

/// The user's profile as text with the years of work counted until now, or None when they have
/// none. The years go on their own line under the header, so the CV's readers take them as
/// given (stated_years_of_work) instead of counting.
pub(crate) async fn text_for_user_at(db: &SqlitePool, now: YearMonth) -> sqlx::Result<Option<String>> {
    let Some(profile) = contact(db).await? else {
        return Ok(None);
    };
    let entries = entries(db).await?;
    let links = links_for_user(db).await?;
    let Contact {
        name,
        email,
        phone,
        title,
        location,
        about,
    } = profile;

    let contact = join_present(&[&location, &phone, &email], " · ");
    let link_line = links
        .iter()
        .map(|(label, url)| format!("{label}: {url}"))
        .collect::<Vec<_>>()
        .join(" · ");
    let years = YearsOfWork {
        saved: saved_years_of_work(db).await?.flatten(),
        counted: counted_years_of_work(&entries, now),
    }
    .years()
    .map(|years| format!("{YEARS_OF_WORK_LABEL}{years}"))
    .unwrap_or_default();
    let mut blocks = vec![join_present(&[&name, &title, &contact, &link_line, &years], "\n")];
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
        let lines = lines(db, kind).await?;
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

/// How the CV text names the years of professional work: profile text writes the line, and
/// stated_years_of_work reads it back so every reader of the CV (the model, the free reader)
/// takes the same number.
pub const YEARS_OF_WORK_LABEL: &str = "Years of professional work: ";

/// Most years of work the profile takes: a working life.
pub const MAX_YEARS_OF_WORK: i64 = 60;

/// The candidate's years of professional work: the number they saved, and the one the app counts
/// from the profile's jobs, which it offers as the suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YearsOfWork {
    /// What the candidate saved, None until they do.
    pub saved: Option<i64>,
    /// Counted from the jobs' dates (counted_years_of_work), None when no job is dated.
    pub counted: Option<i64>,
}

impl YearsOfWork {
    /// The years everything uses: the saved number, else the counted one.
    pub fn years(&self) -> Option<i64> {
        self.saved.or(self.counted)
    }
}

/// The candidate's years of work, saved and counted until now, or None when there is no profile.
pub async fn years_of_work(db: &SqlitePool, now: YearMonth) -> sqlx::Result<Option<YearsOfWork>> {
    let Some(saved) = saved_years_of_work(db).await? else {
        return Ok(None);
    };
    let counted = counted_years_of_work(&entries(db).await?, now);
    Ok(Some(YearsOfWork { saved, counted }))
}

/// The years of work the candidate saved (None until they do), or None when there is no profile.
async fn saved_years_of_work(db: &SqlitePool) -> sqlx::Result<Option<Option<i64>>> {
    sqlx::query_scalar!(r#"SELECT years_of_work FROM profile"#)
        .fetch_optional(db)
        .await
}

/// Whole years of professional work the jobs (the experience entries) cover until now, overlaps
/// counted once, or None when no job is dated. Education and the candidate's own projects aren't
/// work, so they never count: left to count the years themselves from the whole CV, the writing
/// models counted from the earliest date in it and claimed years the candidate hasn't worked.
fn counted_years_of_work(entries: &[Entry], now: YearMonth) -> Option<i64> {
    let jobs: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry.section == "experience" && entry.started_on.is_some())
        .collect();
    if jobs.is_empty() {
        return None;
    }
    Some(free_reader::years_worked(&work_history_dates(&jobs), now))
}

/// The years of work a CV text states on its YEARS_OF_WORK_LABEL line, if it has one.
pub fn stated_years_of_work(cv_text: &str) -> Option<i64> {
    cv_text
        .lines()
        .find_map(|line| line.trim().strip_prefix(YEARS_OF_WORK_LABEL))
        .and_then(|years| years.trim().parse::<i64>().ok())
        .filter(|years| (0..=MAX_YEARS_OF_WORK).contains(years))
}

/// The jobs' date ranges, one per line, the way the free reader reads a work history.
fn work_history_dates(jobs: &[&Entry]) -> String {
    jobs.iter()
        .map(|job| months(job.started_on.as_deref(), job.ended_on.as_deref()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The user's profile links (LinkedIn, GitHub, ...) as (label, url) in their order.
pub async fn links_for_user(db: &SqlitePool) -> sqlx::Result<Vec<(String, String)>> {
    let rows = sqlx::query!(r#"SELECT label, url FROM profile_links ORDER BY position, id"#)
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().map(|row| (row.label, row.url)).collect())
}

/// The parts that aren't empty (nor "0"), joined.
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
async fn entries(db: &SqlitePool) -> sqlx::Result<Vec<Entry>> {
    let rows = sqlx::query_file!("queries/profile/entries.sql").fetch_all(db).await?;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let bullets = sqlx::query_scalar!(
            r#"
            SELECT text
            FROM profile_lines
            WHERE entry_id = ?1 AND kind = 'bullet'
            ORDER BY sort_order, id
            "#,
            row.id
        )
        .fetch_all(db)
        .await?;
        entries.push(Entry {
            section: row.section,
            title: row.title,
            organisation: row.organisation,
            location: row.location,
            url: row.url,
            started_on: row.started_on,
            ended_on: row.ended_on,
            summary: row.summary,
            note: row.note,
            bullets,
        });
    }
    Ok(entries)
}

async fn lines(db: &SqlitePool, kind: &str) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!(
        r#"
        SELECT text
        FROM profile_lines
        WHERE entry_id IS NULL AND kind = ?1
        ORDER BY sort_order, id
        "#,
        kind
    )
    .fetch_all(db)
    .await
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
    use crate::profile::profile_model;
    use sqlx::Executor;

    #[tokio::test]
    async fn the_profile_reads_as_a_cv() {
        let db = db::open_in_memory().await.unwrap();
        db.execute(sqlx::raw_sql(
            r#"
             INSERT INTO profile (id, name, email, phone, title, location, about, created_at, updated_at)
             VALUES (1, 'Jonas', 'me@example.com', '12345678', 'Software developer', 'Slagelse', ' I build things. ', 0, 0);
             INSERT INTO profile_entries (id, section, title, organisation, started_on, ended_on, sort_order, created_at, updated_at)
             VALUES (1, 'experience', 'Developer', 'Old Firm', '2017-01-01', '2020-03-01', 0, 0, 0),
                    (2, 'experience', 'Lead', 'New Firm', '2020-04-01', NULL, 1, 0, 0),
                    (3, 'education', 'Datamatiker', 'Zealand', NULL, '2016-06-01', 0, 0, 0);
             INSERT INTO profile_lines (entry_id, kind, text, sort_order)
             VALUES (2, 'bullet', 'Built the platform', 0), (NULL, 'skill', 'PHP', 0), (NULL, 'skill', 'Rust', 1);
             INSERT INTO profile_links (label, url, position, created_at, updated_at)
             VALUES ('GitHub', 'https://github.com/sasin91', 0, 0, 0);
            "#,
        ))
        .await
        .unwrap();
        assert_eq!(
            text_for_user_at(&db, NOVEMBER_2026).await.unwrap().unwrap(),
            "Jonas\nSoftware developer\nSlagelse · 12345678 · me@example.com\nGitHub: https://github.com/sasin91\n\
             Years of professional work: 9\n\n\
             I build things.\n\n\
             Experience\n\nLead · New Firm\nApril 2020 – now\n- Built the platform\n\n\
             Developer · Old Firm\nJanuary 2017 – March 2020\n\n\
             Education\n\nDatamatiker · Zealand\n – June 2016\n\n\
             Skills\n- PHP\n- Rust"
        );
    }

    /// The month the tests count until.
    const NOVEMBER_2026: YearMonth = YearMonth { year: 2026, month: 11 };

    /// Made up: school until 2014, an own project from 2010, two jobs from January 2017. Only
    /// the jobs count, and the gap and the overlap don't add years; a saved number wins.
    #[tokio::test]
    async fn only_the_jobs_count_as_years_of_work_until_one_is_saved() {
        let db = db::open_in_memory().await.unwrap();
        assert_eq!(years_of_work(&db, NOVEMBER_2026).await.unwrap(), None);
        db.execute(sqlx::raw_sql(
            r#"
             INSERT INTO profile (id, name, email, phone, title, location, about, created_at, updated_at)
             VALUES (1, 'Kim', '', '', '', '', '', 0, 0);
             INSERT INTO profile_entries (id, section, title, organisation, started_on, ended_on, sort_order, created_at, updated_at)
             VALUES (1, 'education', 'Webudvikler', 'School', '2012-08-01', '2014-06-01', 0, 0, 0),
                    (2, 'project', 'Own site', '', '2010-01-01', NULL, 0, 0, 0);
            "#,
        ))
        .await
        .unwrap();
        let none = YearsOfWork {
            saved: None,
            counted: None,
        };
        assert_eq!(years_of_work(&db, NOVEMBER_2026).await.unwrap(), Some(none));
        db.execute(sqlx::raw_sql(
            r#"
             INSERT INTO profile_entries (id, section, title, organisation, started_on, ended_on, sort_order, created_at, updated_at)
             VALUES (3, 'experience', 'Developer', 'Old Firm', '2017-01-01', '2021-06-01', 0, 0, 0),
                    (4, 'experience', 'Lead', 'New Firm', '2021-03-01', NULL, 1, 0, 0);
            "#,
        ))
        .await
        .unwrap();
        let counted = years_of_work(&db, NOVEMBER_2026).await.unwrap().unwrap();
        assert_eq!((counted.counted, counted.years()), (Some(9), Some(9)));
        profile_model::save_years_of_work(&db, Some(8)).await.unwrap();
        let saved = years_of_work(&db, NOVEMBER_2026).await.unwrap().unwrap();
        assert_eq!((saved.saved, saved.counted, saved.years()), (Some(8), Some(9), Some(8)));
        let text = text_for_user_at(&db, NOVEMBER_2026).await.unwrap().unwrap();
        assert_eq!(stated_years_of_work(&text), Some(8));
    }

    #[test]
    fn the_stated_years_are_read_back_from_their_line() {
        assert_eq!(
            stated_years_of_work("Kim\nYears of professional work: 9\n\nExperience"),
            Some(9)
        );
        assert_eq!(stated_years_of_work("I have 16 years of experience"), None);
        assert_eq!(stated_years_of_work("Years of professional work: 400"), None);
    }
}
