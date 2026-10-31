//! The tailored résumé's own match against the post next to the CV's, as the résumé box prints
//! it: both scores, and the résumé's saved
//! match to open.

use super::match_model::SavedMatch;
use super::match_score;

pub const SAVED: &str = "Saved as its own match.";
pub const OPEN: &str = "Open it";

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeComparison {
    /// "The tailored résumé scores 81% good against this post; your CV scored 62% medium."
    pub line: String,
    pub saved: &'static str,
    pub open: &'static str,
    /// The résumé's saved match.
    pub open_id: i64,
}

impl ResumeComparison {
    /// cv_match and resume_match: the saved matches of the CV and of the résumé.
    pub fn from(cv_match: &SavedMatch, resume_match: &SavedMatch) -> Self {
        ResumeComparison {
            line: format!(
                "The tailored résumé scores {} against this post; your CV scored {}.",
                score(resume_match),
                score(cv_match)
            ),
            saved: SAVED,
            open: OPEN,
            open_id: resume_match.id,
        }
    }
}

fn score(found: &SavedMatch) -> String {
    match_score::short_label(found.score, &found.tag)
}
