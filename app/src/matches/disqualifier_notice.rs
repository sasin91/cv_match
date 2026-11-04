//! The likely disqualifiers of a match, as the result prints them above the requirements: where
//! the post asks the candidate to live or show up when they likely don't, a language they don't
//! speak (one the post asks for, or the one the job is done in), and a career level their years
//! don't fit (a student job for a senior). Each line is the post's words and the reason. Such a
//! match is capped just under "medium" (matcher::DISQUALIFIED_INDEX), not hidden.

use super::match_model::SavedItem;
use super::matcher::{self, Groups, Verdicts};

/// The heading and note when the only disqualifier is where the candidate lives.
pub const LOCATION_HEADING: &str = "Likely disqualifier: where you live";
pub const LOCATION_NOTE: &str = "The score is capped below medium for this. Apply anyway if you can move or commute.";
/// The heading and note when the only disqualifier is a language the candidate doesn't speak.
pub const LANGUAGE_HEADING: &str = "Likely disqualifier: a language you don't speak";
pub const LANGUAGE_NOTE: &str =
    "The score is capped below medium for this, and a job scan doesn't write a résumé or application for it.";
/// The heading when the only disqualifier is the post's career level.
pub const LEVEL_HEADING: &str = "Likely disqualifier: the post's level";
/// The heading when there are several kinds.
pub const BOTH_HEADING: &str = "Likely disqualifiers";
/// The note on a level, alone or with others.
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
        let criteria: Vec<&str> = missed.iter().map(|(criterion, _)| *criterion).collect();
        let lines = missed
            .into_iter()
            .map(|(_, item)| DisqualifierLine {
                text: item.text.clone(),
                reason: verdicts.get(&item.id).map(|v| v.reason.clone()).unwrap_or_default(),
            })
            .collect();
        notice(&criteria, lines)
    }

    /// A saved match's items (match_items).
    pub fn from_saved(items: &[SavedItem]) -> Option<Self> {
        let missed: Vec<&SavedItem> = items.iter().filter(|item| is_missed_disqualifier(item)).collect();
        let criteria: Vec<&str> = missed.iter().map(|item| item.criterion.as_str()).collect();
        let lines = missed
            .into_iter()
            .map(|item| DisqualifierLine {
                text: item.text.clone(),
                reason: item.reason.clone(),
            })
            .collect();
        notice(&criteria, lines)
    }
}

/// Whether a saved match item is a disqualifier the candidate missed, one that capped the score: a
/// location or language nobody weighed is saved as partial (ai_match::unweighed).
pub fn is_missed_disqualifier(item: &SavedItem) -> bool {
    matcher::DISQUALIFIERS.contains(&item.criterion.as_str()) && item.verdict == "missing"
}

/// Whether a saved match misses a language: a job scan writes nothing for it then.
pub fn misses_a_language(items: &[SavedItem]) -> bool {
    items
        .iter()
        .any(|item| item.criterion == matcher::LANGUAGE && is_missed_disqualifier(item))
}

/// The notice for the missed lines, under the criterion of each.
fn notice(criteria: &[&str], lines: Vec<DisqualifierLine>) -> Option<DisqualifierNotice> {
    let mut kinds = criteria.to_vec();
    kinds.sort_unstable();
    kinds.dedup();
    let (heading, note) = match kinds.as_slice() {
        [matcher::LOCATION] => (LOCATION_HEADING, LOCATION_NOTE),
        [matcher::LANGUAGE] => (LANGUAGE_HEADING, LANGUAGE_NOTE),
        [matcher::LEVEL] => (LEVEL_HEADING, NOTE),
        _ => (BOTH_HEADING, NOTE),
    };
    (!lines.is_empty()).then_some(DisqualifierNotice { heading, note, lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn missing(criterion: &str, text: &str) -> SavedItem {
        SavedItem {
            criterion: criterion.into(),
            text: text.into(),
            verdict: "missing".into(),
            decided_by: "llm".into(),
            reason: String::new(),
            evidence: String::new(),
        }
    }

    #[test]
    fn a_missed_language_has_its_own_heading() {
        let notice = DisqualifierNotice::from_saved(&[missing(matcher::LANGUAGE, "Finnish")]).unwrap();
        assert_eq!(notice.heading, LANGUAGE_HEADING);
        assert_eq!(notice.note, LANGUAGE_NOTE);
        assert_eq!(notice.lines[0].text, "Finnish");
    }

    #[test]
    fn several_kinds_share_one_heading() {
        let items = [
            missing(matcher::LOCATION, "On site in Hämeenlinna"),
            missing(matcher::LANGUAGE, "Finnish"),
        ];
        let notice = DisqualifierNotice::from_saved(&items).unwrap();
        assert_eq!(notice.heading, BOTH_HEADING);
        assert_eq!(notice.lines.len(), 2);
    }

    #[test]
    fn two_locations_keep_the_location_heading() {
        let items = [
            missing(matcher::LOCATION, "Aarhus"),
            missing(matcher::LOCATION, "Odense"),
        ];
        assert_eq!(
            DisqualifierNotice::from_saved(&items).unwrap().heading,
            LOCATION_HEADING
        );
    }

    #[test]
    fn only_a_missed_language_stops_the_writing() {
        assert!(misses_a_language(&[missing(matcher::LANGUAGE, "Finnish")]));
        assert!(!misses_a_language(&[missing(matcher::LOCATION, "Aarhus")]));
        let spoken = SavedItem {
            verdict: "met".into(),
            ..missing(matcher::LANGUAGE, "Danish")
        };
        assert!(!misses_a_language(&[spoken]));
    }
}
