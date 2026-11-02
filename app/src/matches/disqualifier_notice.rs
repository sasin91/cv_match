//! The likely disqualifiers of a match, as the result prints them above the requirements: where
//! the post asks the candidate to live or show up when they likely don't, and a career level their
//! years don't fit (a student job for a senior). Each line is the post's words and the reason.
//! Such a match is capped just under "medium" (matcher::DISQUALIFIED_INDEX), not hidden.

use super::match_model::SavedItem;
use super::matcher::{self, Groups, Verdicts};

/// The heading and note when the only disqualifier is where the candidate lives.
pub const LOCATION_HEADING: &str = "Likely disqualifier: where you live";
pub const LOCATION_NOTE: &str = "The score is capped below medium for this. Apply anyway if you can move or commute.";
/// The heading when the only disqualifier is the post's career level.
pub const LEVEL_HEADING: &str = "Likely disqualifier: the post's level";
/// The heading when there are both.
pub const BOTH_HEADING: &str = "Likely disqualifiers";
/// The note on a level, alone or with a location.
pub const NOTE: &str = "The score is capped below medium for this.";

#[derive(Debug, Clone, PartialEq)]
pub struct DisqualifierLine {
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisqualifierNotice {
    pub heading: &'static str,
    pub note: &'static str,
    pub lines: Vec<DisqualifierLine>,
}

impl DisqualifierNotice {
    /// A match just made: its groups (matcher::criteria) and combined verdicts.
    pub fn from(groups: &Groups, verdicts: &Verdicts) -> Option<Self> {
        let missed = matcher::missed_disqualifiers(groups, verdicts);
        let level = missed.iter().any(|item| item.kind == matcher::LEVEL);
        let lines = missed
            .into_iter()
            .map(|item| DisqualifierLine {
                text: item.text.clone(),
                reason: verdicts.get(&item.id).map(|v| v.reason.clone()).unwrap_or_default(),
            })
            .collect();
        notice(level, lines)
    }

    /// A saved match's items (match_items).
    pub fn from_saved(items: &[SavedItem]) -> Option<Self> {
        let missed: Vec<&SavedItem> = items
            .iter()
            .filter(|item| matcher::DISQUALIFIERS.contains(&item.criterion.as_str()) && item.verdict == "missing")
            .collect();
        let level = missed.iter().any(|item| item.criterion == matcher::LEVEL);
        let lines = missed
            .into_iter()
            .map(|item| DisqualifierLine {
                text: item.text.clone(),
                reason: item.reason.clone(),
            })
            .collect();
        notice(level, lines)
    }
}

/// The notice for the missed lines; level says whether one of them is the post's career level.
fn notice(level: bool, lines: Vec<DisqualifierLine>) -> Option<DisqualifierNotice> {
    let (heading, note) = match (level, lines.len()) {
        (false, _) => (LOCATION_HEADING, LOCATION_NOTE),
        (true, 1) => (LEVEL_HEADING, NOTE),
        (true, _) => (BOTH_HEADING, NOTE),
    };
    (!lines.is_empty()).then_some(DisqualifierNotice { heading, note, lines })
}
