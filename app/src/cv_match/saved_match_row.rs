//! A saved match as a row of the page's list (templates/cv_match/history.html), ported from
//! modules/cv_match/Saved_match_row.php: its id, tag, title and meta line, ready to print.

use super::cv_match_model::{APPLIED_STATUSES, ListedMatch};
use super::{dates, match_score, reject_reason};

/// How a match of a tailored résumé is named (its cv_name ends with this).
pub const TAILORED_CV: &str = "(tailored résumé)";

#[derive(Debug, Clone, PartialEq)]
pub struct SavedMatchRow {
    pub id: i64,
    pub tag: String,
    pub title: String,
    /// "81% good · likely disqualifier: Located close to Copenhagen".
    pub about: String,
    pub saved_at: String,
    pub saved_on: String,
    /// "application · applied · on Jobnet", ending "rejected: Too long commute" when rejected.
    pub done: String,
}

impl SavedMatchRow {
    pub fn from(m: &ListedMatch) -> Self {
        let mut about = vec![match_score::short_label(m.score, &m.tag)];
        if m.cv_name.ends_with(TAILORED_CV) {
            about.push("tailored résumé".to_string());
        }
        if let Some(location) = &m.location_missed {
            about.push(format!("likely disqualifier: {location}"));
        }
        SavedMatchRow {
            id: m.id,
            tag: m.tag.clone(),
            title: title(m),
            about: about.join(" · "),
            saved_at: dates::iso(m.created_at),
            saved_on: dates::day(m.created_at),
            done: done(m).join(" · "),
        }
    }
}

/// "Job title · Company", or "Untitled" when the post named neither.
fn title(m: &ListedMatch) -> String {
    let parts: Vec<&str> = [m.job_title.as_str(), m.company.as_str()]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        "Untitled".to_string()
    } else {
        parts.join(" · ")
    }
}

/// What has been done for the match: written application, résumé, applied, logged on Jobnet,
/// rejected and why.
fn done(m: &ListedMatch) -> Vec<String> {
    let applied = m
        .jobnet_status
        .as_deref()
        .is_some_and(|status| APPLIED_STATUSES.contains(&status));
    let mut done: Vec<String> = [
        ("application", m.has_application),
        ("résumé", m.has_resume),
        ("applied", applied),
        ("on Jobnet", m.jobnet_logged_at.is_some()),
    ]
    .into_iter()
    .filter(|(_, is)| *is)
    .map(|(label, _)| label.to_string())
    .collect();
    if m.rejected_at.is_some() {
        let why = reject_reason::describe(&m.rejections);
        done.push(if why.is_empty() {
            "rejected".to_string()
        } else {
            format!("rejected: {why}")
        });
    }
    done
}
