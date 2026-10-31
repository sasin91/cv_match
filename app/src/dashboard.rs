//! The dashboard at the top of the match page: what the app runs on and what is missing, each
//! with a link to set it up, and a glance at the saved matches. It is the page the app opens on,
//! like a security suite's home screen: the only thing a launch checks is whether an AI model is
//! set up, and that shows as an alert, not a redirect, since the free keyword match works
//! without one.
//!
//! The rows come from setup_steps, the same functions the setup wizard shows, so the two (and a
//! native UI later) can't disagree.

use crate::matches::llm_key;
use crate::matches::match_model::{self, LISTS, SHORTLIST};
use crate::matches::saved_match_row::SavedMatchRow;
use crate::server::AppState;
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

/// One thing the app runs on, and how it stands.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRow {
    pub name: &'static str,
    pub health: Health,
    pub text: String,
    pub fix: Option<Fix>,
}

/// A list of saved matches, and how many it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct ListCount {
    pub href: String,
    pub label: &'static str,
    pub count: i64,
}

/// The dashboard as the page prints it.
pub struct Dashboard {
    /// No AI model is set up: the page's alert.
    pub ai_missing: bool,
    /// "Everything is set up." or "2 things need your attention."
    pub headline: String,
    pub all_ready: bool,
    pub rows: Vec<StatusRow>,
    pub counts: Vec<ListCount>,
    /// The shortlist's best few.
    pub best: Vec<SavedMatchRow>,
}

pub async fn dashboard(state: &AppState) -> Result<Dashboard, String> {
    let database = |e: sqlx::Error| format!("The database couldn't answer: {e}");
    let ai = setup_steps::ai(state).await.map_err(database)?;
    let uses = llm_key::ai_uses(state).await.map_err(|e| e.message)?;
    let cv = setup_steps::cv(state).await?;
    let schedule = setup_steps::schedule(state).await;
    let rows = vec![
        ai_row(&ai.saved, uses.any()),
        cv_row(cv.filled.as_ref()),
        laya_row(state),
        scan_row(schedule.interval_hours, schedule.error.as_deref()),
        extension_row(&state.extension_folder),
    ];
    let counts = match_model::list_counts(state.db(), state.user_id)
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
    let best = match_model::best_first(state.db(), state.user_id, BEST_SHOWN, "", SHORTLIST)
        .await
        .map_err(database)?
        .rows
        .iter()
        .map(SavedMatchRow::from)
        .collect();
    let attention = rows.iter().filter(|row| row.health == Health::Attention).count();
    Ok(Dashboard {
        ai_missing: !uses.any(),
        headline: headline(attention),
        all_ready: attention == 0,
        rows,
        counts,
        best,
    })
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
    }
}

fn cv_row(filled: Option<&crate::profile::profile_model::FilledFrom>) -> StatusRow {
    match filled {
        Some(filled) => StatusRow {
            name: "Your profile",
            health: Health::Ready,
            text: setup_steps::filled_line(filled),
            fix: None,
        },
        None => StatusRow {
            name: "Your profile",
            health: Health::Attention,
            text: "None yet, so each match needs a CV pasted in.".to_string(),
            fix: Some(Fix {
                href: "setup#cv",
                label: "Read your CV",
            }),
        },
    }
}

fn laya_row(state: &AppState) -> StatusRow {
    match &state.laya {
        Some(laya) => StatusRow {
            name: "Laya",
            health: Health::Ready,
            text: format!("Its model is in {}.", laya.folder().display()),
            fix: None,
        },
        None => StatusRow {
            name: "Laya",
            health: Health::Neutral,
            text: "No model found, so matches go on without Laya's line.".to_string(),
            fix: None,
        },
    }
}

fn scan_row(interval_hours: Option<u32>, error: Option<&str>) -> StatusRow {
    let fix = Some(Fix {
        href: "setup#schedule",
        label: "Change it",
    });
    let (health, text) = match (interval_hours, error) {
        (_, Some(why)) => (Health::Attention, format!("Task Scheduler couldn't be read: {why}")),
        (Some(hours), None) => (
            Health::Attention,
            format!(
                "Scheduled {}, but the scan itself isn't in the app yet, so each run finds nothing.",
                setup_steps::interval_label(hours).to_lowercase()
            ),
        ),
        (None, None) => (
            Health::Neutral,
            "Not scheduled. The scan itself isn't in the app yet.".to_string(),
        ),
    };
    StatusRow {
        name: "Job scan",
        health,
        text,
        fix,
    }
}

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
        },
        Err(why) => StatusRow {
            name: "Jobnet extension",
            health: Health::Attention,
            text: why.clone(),
            fix: Some(Fix {
                href: "joblog_extension",
                label: "See why",
            }),
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

    #[test]
    fn a_scheduled_scan_says_it_isnt_built_yet() {
        let scheduled = scan_row(Some(4), None);
        assert_eq!(scheduled.health, Health::Attention);
        assert!(
            scheduled.text.starts_with("Scheduled every 4 hours, but"),
            "{}",
            scheduled.text
        );
        assert_eq!(scan_row(None, None).health, Health::Neutral);
    }
}
