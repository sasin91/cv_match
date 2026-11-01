//! The dashboard at the top of the match page: what the app runs on and what is missing, each
//! with a link to set it up, and a glance at the saved matches. It is the page the app opens on,
//! like a security suite's home screen: the only thing a launch checks is whether an AI model is
//! set up, and that shows as an alert, not a redirect, since the free keyword match works
//! without one.
//!
//! The rows come from setup_steps, the same functions the setup wizard shows, so the two (and a
//! native UI later) can't disagree.

use crate::context::Context;
use crate::job_scan::scan_service::{self, ScanStatus};
use crate::matches::match_model::{self, LISTS, SHORTLIST};
use crate::matches::saved_match_row::SavedMatchRow;
use crate::matches::{dates, llm_key};
use crate::setup::setup_steps;

/// How many of the shortlist's best matches the dashboard shows.
const BEST_SHOWN: usize = 3;

/// How a status row stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Health {
    /// Set up and working.
    Ready,
    /// Missing or not working, and the user should do something.
    Attention,
    /// Off, or only for the user to know: nothing is wrong.
    Neutral,
}

impl Health {
    /// The row's CSS class.
    pub fn class(&self) -> &'static str {
        match self {
            Health::Ready => "ready",
            Health::Attention => "attention",
            Health::Neutral => "neutral",
        }
    }

    /// What a screen reader says before the row.
    pub fn label(&self) -> &'static str {
        match self {
            Health::Ready => "Ready",
            Health::Attention => "Needs attention",
            Health::Neutral => "Note",
        }
    }
}

/// A link that sets a row up.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub href: &'static str,
    pub label: &'static str,
}

/// A button a row does its work with: a form posted to action, coming back to the dashboard.
#[derive(Debug, Clone, PartialEq)]
pub struct RowAction {
    pub action: &'static str,
    pub label: &'static str,
}

/// One thing the app runs on, and how it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRow {
    pub name: &'static str,
    pub health: Health,
    pub text: String,
    pub fix: Option<Fix>,
    pub action: Option<RowAction>,
}

/// A list of saved matches, and how many it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct ListCount {
    pub href: String,
    pub label: &'static str,
    pub count: i64,
}

/// The dashboard's status, as the page prints it.
pub struct Dashboard {
    /// No AI model is set up: the page's alert.
    pub ai_missing: bool,
    /// "Everything is set up." or "2 things need your attention."
    pub headline: String,
    pub all_ready: bool,
    pub rows: Vec<StatusRow>,
}

/// The glance at the saved matches, which the page reads again with the saved matches' list
/// (match/history_list), so it keeps up with matches saved, rejected or logged on the page.
pub struct Findings {
    pub counts: Vec<ListCount>,
    /// The shortlist's best few.
    pub best: Vec<SavedMatchRow>,
}

/// What the database said, for the page.
fn database(e: sqlx::Error) -> String {
    format!("The database couldn't answer: {e}")
}

pub async fn dashboard(context: &Context) -> Result<Dashboard, String> {
    let ai = setup_steps::ai(context).await.map_err(database)?;
    let uses = llm_key::ai_uses(context).await.map_err(|e| e.message)?;
    let cv = setup_steps::cv(context).await?;
    let schedule = setup_steps::schedule(context).await;
    let scan = scan_service::status(context).await.map_err(|e| e.message)?;
    let rows = vec![
        ai_row(&ai.saved, uses.any()),
        cv_row(cv.filled.as_ref()),
        laya_row(context),
        scan_row(schedule.interval_hours, schedule.error.as_deref(), &scan),
        extension_row(&context.extension_folder),
    ];
    let attention = rows.iter().filter(|row| row.health == Health::Attention).count();
    Ok(Dashboard {
        ai_missing: !uses.any(),
        headline: headline(attention),
        all_ready: attention == 0,
        rows,
    })
}

pub async fn findings(context: &Context) -> Result<Findings, String> {
    let counts = match_model::list_counts(context.db(), context.user_id)
        .await
        .map_err(database)?
        .into_iter()
        .map(|(list, count)| ListCount {
            href: list_href(list),
            label: LISTS
                .iter()
                .find(|(name, ..)| *name == list)
                .map(|(_, label, _)| *label)
                .unwrap_or(list),
            count,
        })
        .collect();
    let best = match_model::best_first(context.db(), context.user_id, BEST_SHOWN, "", SHORTLIST)
        .await
        .map_err(database)?
        .rows
        .iter()
        .map(SavedMatchRow::from)
        .collect();
    Ok(Findings { counts, best })
}

/// The match page's link to a list.
fn list_href(list: &str) -> String {
    if list == SHORTLIST {
        "match".to_string()
    } else {
        format!("match?list={list}")
    }
}

fn headline(attention: usize) -> String {
    match attention {
        0 => "Everything is set up.".to_string(),
        1 => "1 thing needs your attention.".to_string(),
        n => format!("{n} things need your attention."),
    }
}

fn ai_row(saved: &[String], in_use: bool) -> StatusRow {
    let (health, text, fix) = if saved.is_empty() {
        (
            Health::Attention,
            "None set up, so matches use the free keyword match and nothing is written for you.".to_string(),
            Some(Fix {
                href: "setup#ai",
                label: "Set one up",
            }),
        )
    } else if !in_use {
        (
            Health::Attention,
            format!(
                "{} saved, but not picked for anything, so matches use the free keyword match.",
                saved.join("; ")
            ),
            Some(Fix {
                href: "account",
                label: "Pick what it's used for",
            }),
        )
    } else {
        (Health::Ready, saved.join("; "), None)
    };
    StatusRow {
        name: "AI model",
        health,
        text,
        fix,
        action: None,
    }
}

fn cv_row(filled: Option<&crate::profile::profile_model::FilledFrom>) -> StatusRow {
    match filled {
        Some(filled) => StatusRow {
            name: "Your profile",
            health: Health::Ready,
            text: setup_steps::filled_line(filled),
            fix: None,
            action: None,
        },
        None => StatusRow {
            name: "Your profile",
            health: Health::Attention,
            text: "None yet, so each match needs a CV pasted in.".to_string(),
            fix: Some(Fix {
                href: "setup#cv",
                label: "Read your CV",
            }),
            action: None,
        },
    }
}

fn laya_row(context: &Context) -> StatusRow {
    match &context.laya {
        Some(laya) => StatusRow {
            name: "Laya",
            health: Health::Ready,
            text: format!("Its model is in {}.", laya.folder().display()),
            fix: None,
            action: None,
        },
        None => StatusRow {
            name: "Laya",
            health: Health::Neutral,
            text: "No model found, so matches go on without Laya's line.".to_string(),
            fix: None,
            action: None,
        },
    }
}

/// The job scan: whether it is scheduled, and how its last scan went, with Scan now, which runs
/// the scan the schedule runs.
fn scan_row(interval_hours: Option<u32>, error: Option<&str>, scan: &ScanStatus) -> StatusRow {
    let schedule = match interval_hours {
        Some(hours) => format!("Scheduled {}.", setup_steps::interval_label(hours).to_lowercase()),
        None => "Not scheduled, so it scans only with Scan now.".to_string(),
    };
    let last = match &scan.last_scan {
        Some(last) if scan.running => format!(" Scanning now (started {}).", dates::short_moment(last.started_at)),
        Some(last) => format!(
            " Last scan {}: {} {} new posts in the last day.",
            dates::short_moment(last.started_at),
            last.text,
            scan.new_last_day
        ),
        None => " Not scanned yet.".to_string(),
    };
    let failed = scan.last_scan.as_ref().is_some_and(|last| last.failed && !scan.running);
    let (health, text, fix) = if let Some(why) = error {
        (
            Health::Attention,
            format!("Task Scheduler couldn't be read: {why}"),
            Some(SCHEDULE_FIX),
        )
    } else if scan.active_searches == 0 {
        (
            if interval_hours.is_some() {
                Health::Attention
            } else {
                Health::Neutral
            },
            format!("{schedule} There is no job search to scan yet."),
            Some(Fix {
                href: "job_scan/create",
                label: "Make a job search",
            }),
        )
    } else if let Some(problem) = &scan.problem {
        (Health::Attention, format!("{schedule} {problem}"), Some(SEARCHES_FIX))
    } else if failed {
        (Health::Attention, format!("{schedule}{last}"), Some(SEARCHES_FIX))
    } else if interval_hours.is_none() {
        (Health::Neutral, format!("{schedule}{last}"), Some(SCHEDULE_FIX))
    } else {
        (Health::Ready, format!("{schedule}{last}"), Some(SEARCHES_FIX))
    };
    let can_scan = scan.active_searches > 0 && scan.problem.is_none() && !scan.running;
    StatusRow {
        name: "Job scan",
        health,
        text,
        fix,
        action: can_scan.then_some(RowAction {
            action: "job_scan/submit_scan",
            label: "Scan now",
        }),
    }
}

/// The link to the job scan's schedule.
const SCHEDULE_FIX: Fix = Fix {
    href: "setup#schedule",
    label: "Change the schedule",
};

/// The link to the job searches.
const SEARCHES_FIX: Fix = Fix {
    href: "job_scan",
    label: "Your job searches",
};

fn extension_row(folder: &Result<std::path::PathBuf, String>) -> StatusRow {
    match folder {
        // The app can't see whether the browser has loaded it, so this is a note, not a tick.
        Ok(folder) => StatusRow {
            name: "Jobnet extension",
            health: Health::Neutral,
            text: format!("Its files are in {}. Load it once in Edge.", folder.display()),
            fix: Some(Fix {
                href: "joblog_extension",
                label: "How to load it",
            }),
            action: None,
        },
        Err(why) => StatusRow {
            name: "Jobnet extension",
            health: Health::Attention,
            text: why.clone(),
            fix: Some(Fix {
                href: "joblog_extension",
                label: "See why",
            }),
            action: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_headline_counts_what_needs_attention() {
        assert_eq!(headline(0), "Everything is set up.");
        assert_eq!(headline(1), "1 thing needs your attention.");
        assert_eq!(headline(3), "3 things need your attention.");
    }

    #[test]
    fn a_saved_key_that_isnt_used_still_needs_attention() {
        assert_eq!(ai_row(&[], false).health, Health::Attention);
        let unused = ai_row(&["OpenAI, gpt-5.4-mini".to_string()], false);
        assert_eq!(unused.health, Health::Attention);
        assert_eq!(unused.fix.map(|fix| fix.href), Some("account"));
        assert_eq!(
            ai_row(&["OpenAI, gpt-5.4-mini".to_string()], true).health,
            Health::Ready
        );
    }

    fn scanned(active_searches: i64) -> ScanStatus {
        ScanStatus {
            active_searches,
            last_scan: Some(scan_service::LatestScan {
                started_at: 0,
                text: "found 12, 3 new.".to_string(),
                failed: false,
            }),
            new_last_day: 3,
            ..ScanStatus::default()
        }
    }

    #[test]
    fn a_scheduled_scan_says_how_its_last_scan_went() {
        let row = scan_row(Some(4), None, &scanned(1));
        assert_eq!(row.health, Health::Ready);
        assert!(
            row.text.starts_with("Scheduled every 4 hours. Last scan "),
            "{}",
            row.text
        );
        assert!(
            row.text.ends_with(": found 12, 3 new. 3 new posts in the last day."),
            "{}",
            row.text
        );
        assert_eq!(row.action.map(|action| action.label), Some("Scan now"));
    }

    #[test]
    fn scan_now_waits_for_a_search_a_key_and_the_running_scan() {
        let none = scan_row(Some(4), None, &scanned(0));
        assert_eq!(none.health, Health::Attention);
        assert_eq!(none.fix.map(|fix| fix.href), Some("job_scan/create"));
        assert_eq!(none.action, None);
        let problem = ScanStatus {
            problem: Some("Pick your OpenAI key.".to_string()),
            ..scanned(1)
        };
        let row = scan_row(Some(4), None, &problem);
        assert_eq!(row.health, Health::Attention);
        assert!(row.text.ends_with("Pick your OpenAI key."), "{}", row.text);
        assert_eq!(row.action, None);
        let running = ScanStatus {
            running: true,
            ..scanned(1)
        };
        let row = scan_row(Some(4), None, &running);
        assert!(row.text.contains("Scanning now"), "{}", row.text);
        assert_eq!(row.action, None);
        assert_eq!(scan_row(None, None, &scanned(1)).health, Health::Neutral);
    }
}
