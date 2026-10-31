//! The dashboard at the top of the match page (templates/match/dashboard.html): what is ready,
//! what is missing and a glance at the matches found. Nothing here is required to use the page;
//! each row that can be fixed links to the page that does it.

use super::match_model::{self, LISTS, SHORTLIST};
use super::page_error::PageError;
use super::pages::list_tabs;
use super::saved_match_row::SavedMatchRow;
use crate::server::AppState;
use crate::setup::{self, setup_steps};

/// The best matches the dashboard shows.
const BEST_MATCHES: usize = 3;

/// Most matches a list's count goes up to; more than the app is expected to ever hold.
const MAX_COUNTED: usize = 10_000;

/// How a status row stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusState {
    Ready,
    Missing,
    /// Optional or not running, which nothing depends on.
    Off,
    /// The app can't tell.
    Unknown,
}

impl StatusState {
    /// The CSS class of the row (css/match.css "dashboard").
    pub fn class(&self) -> &'static str {
        match self {
            StatusState::Ready => "ready",
            StatusState::Missing => "missing",
            StatusState::Off => "off",
            StatusState::Unknown => "unknown",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            StatusState::Ready => "Ready",
            StatusState::Missing => "Missing",
            StatusState::Off => "Off",
            StatusState::Unknown => "Can't tell",
        }
    }
}

/// A link to where a status row is fixed.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusFix {
    pub href: &'static str,
    pub label: &'static str,
}

/// A row of the status panel.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusRow {
    pub name: &'static str,
    pub state: StatusState,
    pub text: String,
    pub fix: Option<StatusFix>,
}

/// A list of saved matches and how many it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct ListCount {
    pub href: String,
    pub label: &'static str,
    pub count: usize,
}

pub struct Dashboard {
    pub status: Vec<StatusRow>,
    pub lists: Vec<ListCount>,
    pub best: Vec<SavedMatchRow>,
}

impl Dashboard {
    /// The dashboard as things stand; ai_ready as llm_key::ai_uses says.
    pub async fn load(state: &AppState, ai_ready: bool) -> Result<Self, PageError> {
        Ok(Dashboard {
            status: vec![
                ai_row(state, ai_ready).await?,
                cv_row(state).await,
                laya_row(state),
                scan_row(state).await,
                extension_row(state),
            ],
            lists: list_counts(state).await?,
            best: best_matches(state).await?,
        })
    }
}

async fn ai_row(state: &AppState, ai_ready: bool) -> Result<StatusRow, PageError> {
    let row = if ai_ready {
        StatusRow {
            name: "AI model",
            state: StatusState::Ready,
            text: setup_steps::saved_ai(state).await?.join(" · "),
            fix: Some(StatusFix {
                href: "account",
                label: "Change",
            }),
        }
    } else {
        StatusRow {
            name: "AI model",
            state: StatusState::Missing,
            text: "None set up, so matches use the free keyword match.".to_string(),
            fix: Some(StatusFix {
                href: "setup#ai",
                label: "Set one up",
            }),
        }
    };
    Ok(row)
}

async fn cv_row(state: &AppState) -> StatusRow {
    let (row_state, text, fix_label) = match setup_steps::cv(state).await {
        Ok(step) => match step.filled {
            Some(filled) => (StatusState::Ready, setup::filled_line(&filled), "Read another"),
            None => (
                StatusState::Missing,
                "No profile yet, so each match needs a CV pasted in.".to_string(),
                "Read your CV",
            ),
        },
        Err(why) => {
            eprintln!("dashboard: the profile couldn't be read: {why}");
            (StatusState::Unknown, why, "Open the setup")
        }
    };
    StatusRow {
        name: "Your CV",
        state: row_state,
        text,
        fix: Some(StatusFix {
            href: "setup#cv",
            label: fix_label,
        }),
    }
}

fn laya_row(state: &AppState) -> StatusRow {
    let (row_state, text) = match &state.laya {
        Some(laya) => (
            StatusState::Ready,
            format!("Its model is in {}.", laya.folder().display()),
        ),
        None => (
            StatusState::Off,
            "No model found, so matches go on without Laya.".to_string(),
        ),
    };
    StatusRow {
        name: "Laya",
        state: row_state,
        text,
        fix: None,
    }
}

/// Never Ready: the scan itself isn't ported, so a registered task finds nothing yet.
async fn scan_row(state: &AppState) -> StatusRow {
    /// What the row adds until the scan is ported.
    const NOT_PORTED: &str = "The scan itself isn't in the desktop app yet.";
    let step = setup_steps::schedule(state).await;
    let (row_state, text, fix_label) = match (step.error, step.interval_hours) {
        (Some(why), _) => (StatusState::Unknown, format!("{why} {NOT_PORTED}"), "Open the schedule"),
        (None, Some(hours)) => (
            StatusState::Off,
            format!(
                "Scheduled {}. {NOT_PORTED}",
                setup::interval_label(hours).to_lowercase()
            ),
            "Change",
        ),
        (None, None) => (StatusState::Off, format!("Not scheduled. {NOT_PORTED}"), "Schedule it"),
    };
    StatusRow {
        name: "Job scan",
        state: row_state,
        text,
        fix: Some(StatusFix {
            href: "setup#schedule",
            label: fix_label,
        }),
    }
}

/// Unknown at best: the app writes the extension's files, but can't see whether Edge loaded them.
fn extension_row(state: &AppState) -> StatusRow {
    let (row_state, text) = match &state.extension_folder {
        Ok(folder) => (
            StatusState::Unknown,
            format!(
                "Its files are in {}. The app can't see whether your browser has loaded them.",
                folder.display()
            ),
        ),
        Err(why) => (StatusState::Missing, why.clone()),
    };
    StatusRow {
        name: "Jobnet extension",
        state: row_state,
        text,
        fix: Some(StatusFix {
            href: "joblog_extension",
            label: "Set it up",
        }),
    }
}

/// How many matches each list holds, counted through the query that lists them, so a count can't
/// disagree with its list.
async fn list_counts(state: &AppState) -> Result<Vec<ListCount>, PageError> {
    let mut counts = Vec::with_capacity(LISTS.len());
    for (&(list, ..), tab) in LISTS.iter().zip(list_tabs(SHORTLIST)) {
        let found = match_model::best_first(state.db(), state.user_id, MAX_COUNTED, "", list).await?;
        counts.push(ListCount {
            href: tab.href,
            label: tab.label,
            count: found.rows.len(),
        });
    }
    Ok(counts)
}

async fn best_matches(state: &AppState) -> Result<Vec<SavedMatchRow>, PageError> {
    let found = match_model::best_first(state.db(), state.user_id, BEST_MATCHES, "", SHORTLIST).await?;
    Ok(found.rows.iter().map(SavedMatchRow::from).collect())
}
