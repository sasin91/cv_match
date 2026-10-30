//! The Reject part of a saved match (templates/cv_match/result.html), ported from
//! modules/cv_match/Reject_button.php: "Not interested" opens a form asking why (as many reasons
//! as apply) with room for a note; a rejected match says why it was rejected and offers Bring
//! back. Plain form posts to cv_match/submit_reject/{id}.

use super::reject_reason::{self, Rejection};

pub const OPEN: &str = "Not interested";
pub const REJECT: &str = "Reject";
pub const NOTE_PLACEHOLDER: &str = "Note (optional)";
pub const OTHER_PLACEHOLDER: &str = "What is it? (needed for Other)";
pub const BRING_BACK: &str = "Bring back";
pub const REJECTED: &str = "You rejected this match";
pub const REJECTED_WHERE: &str = "It shows only in the Rejected list.";

/// One reason's checkbox and note.
#[derive(Debug, Clone, PartialEq)]
pub struct ReasonField {
    pub value: &'static str,
    pub label: &'static str,
    pub placeholder: &'static str,
}

/// The reject form of a match not rejected, or the Bring back form of a rejected one.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectButton {
    pub action: String,
    pub rejected: bool,
    pub open: &'static str,
    pub reasons: Vec<ReasonField>,
    pub note_max: usize,
    /// "You rejected this match (Too long commute (1h each way)). It shows only in the Rejected list."
    pub note: String,
    pub submit: &'static str,
}

impl RejectButton {
    /// None for a match that isn't saved. rejected: the match's rejections when it is rejected.
    pub fn from(saved_id: Option<i64>, rejected: Option<&[Rejection]>) -> Option<Self> {
        let action = format!("cv_match/submit_reject/{}", saved_id?);
        Some(match rejected {
            None => RejectButton {
                action,
                rejected: false,
                open: OPEN,
                reasons: reasons(),
                note_max: reject_reason::NOTE_MAX,
                note: String::new(),
                submit: REJECT,
            },
            Some(rejections) => {
                let why = reject_reason::describe(rejections);
                let why = if why.is_empty() {
                    String::new()
                } else {
                    format!(" ({why})")
                };
                RejectButton {
                    action,
                    rejected: true,
                    open: OPEN,
                    reasons: Vec::new(),
                    note_max: reject_reason::NOTE_MAX,
                    note: format!("{REJECTED}{why}. {REJECTED_WHERE}"),
                    submit: BRING_BACK,
                }
            }
        })
    }
}

fn reasons() -> Vec<ReasonField> {
    reject_reason::REASONS
        .iter()
        .map(|&(value, label)| ReasonField {
            value,
            label,
            placeholder: if value == "other" {
                OTHER_PLACEHOLDER
            } else {
                NOTE_PLACEHOLDER
            },
        })
        .collect()
}
