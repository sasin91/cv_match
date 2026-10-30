//! Where the post asks the candidate to live or show up, when they likely don't, as the result
//! prints it above the requirements, ported from modules/cv_match/Location_notice.php: the
//! post's words and the reason. Such a match is capped just under "medium"
//! (cv_matcher::LOCATION_MISSED_INDEX), not hidden.

use super::cv_match_model::SavedItem;
use super::cv_matcher::{self, Groups, Verdicts};

pub const HEADING: &str = "Likely disqualifier: where you live";
pub const NOTE: &str = "The score is capped below medium for this. Apply anyway if you can move or commute.";

#[derive(Debug, Clone, PartialEq)]
pub struct LocationLine {
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocationNotice {
    pub heading: &'static str,
    pub note: &'static str,
    pub lines: Vec<LocationLine>,
}

impl LocationNotice {
    /// A match just made: its groups (cv_matcher::criteria) and combined verdicts.
    pub fn from(groups: &Groups, verdicts: &Verdicts) -> Option<Self> {
        notice(
            cv_matcher::missed_locations(groups, verdicts)
                .into_iter()
                .map(|item| LocationLine {
                    text: item.text.clone(),
                    reason: verdicts.get(&item.id).map(|v| v.reason.clone()).unwrap_or_default(),
                })
                .collect(),
        )
    }

    /// A saved match's items (cv_match_items).
    pub fn from_saved(items: &[SavedItem]) -> Option<Self> {
        notice(
            items
                .iter()
                .filter(|item| item.criterion == cv_matcher::LOCATION && item.verdict == "missing")
                .map(|item| LocationLine {
                    text: item.text.clone(),
                    reason: item.reason.clone(),
                })
                .collect(),
        )
    }
}

fn notice(lines: Vec<LocationLine>) -> Option<LocationNotice> {
    (!lines.is_empty()).then_some(LocationNotice {
        heading: HEADING,
        note: NOTE,
        lines,
    })
}
