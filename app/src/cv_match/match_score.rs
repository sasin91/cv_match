//! A match score as the pages show it, ported from modules/cv_match/Match_score.php: the 0..1
//! score as a percent, and the legacy tag (cv_matcher::match_tag) with its name. Every page that
//! prints a score gets it from here.

use crate::php;

/// What a stored 0..1 score is multiplied by to read as a percent.
const PERCENT: f64 = 100.0;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MatchScore {
    pub percent: i64,
    pub tag: String,
    pub tag_name: String,
}

impl MatchScore {
    /// The score and tag as printed, or None when there is no score yet.
    pub fn from(score: Option<f64>, tag: &str) -> Option<Self> {
        score.map(|score| Self {
            percent: percent(score),
            tag: tag.to_string(),
            tag_name: tag_name(tag).to_string(),
        })
    }
}

/// 0.814 -> 81.
pub fn percent(score: f64) -> i64 {
    php::round(score * PERCENT) as i64
}

/// "Good match" for good; a tag it doesn't know as it is.
pub fn tag_name(tag: &str) -> &str {
    match tag {
        "top" => "Top match",
        "good" => "Good match",
        "medium" => "Medium match",
        "poor" => "Weak match",
        other => other,
    }
}

/// "81% good".
pub fn short_label(score: f64, tag: &str) -> String {
    format!("{}% {tag}", percent(score))
}
