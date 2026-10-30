//! One requirement of a match with its verdict, as the result prints it, ported from
//! modules/cv_match/Verdict_row.php: the verdict, the requirement, why, the evidence quoted from
//! the CV, what decided it and, for a requirement met by any of several, each alternative as a
//! row of its own.

use super::cv_match_model::SavedItem;
use super::cv_matcher::{Item, Verdict, Verdicts};

/// The verdicts, missing first, then partial, then met.
pub const ORDER: [&str; 3] = ["missing", "partial", "met"];

/// What a requirement without a verdict shows.
const NO_VERDICT_BY: &str = "llm";
const NO_VERDICT_REASON: &str = "No verdict returned.";

#[derive(Debug, Clone, PartialEq)]
pub struct VerdictRow {
    pub verdict: &'static str,
    pub text: String,
    pub reason: String,
    pub evidence: String,
    /// "model", "rule", "keyword match" or "taxonomy, exact".
    pub decided_by: String,
    pub alternatives: Vec<VerdictRow>,
}

impl VerdictRow {
    /// A fresh match's item (cv_matcher::criteria) with the verdicts by item id
    /// (cv_matcher::combine).
    pub fn from(item: &Item, verdicts: &Verdicts) -> Self {
        let no_verdict = Verdict {
            verdict: "missing".to_string(),
            by: NO_VERDICT_BY.to_string(),
            method: String::new(),
            evidence: String::new(),
            reason: NO_VERDICT_REASON.to_string(),
        };
        let verdict = verdicts.get(&item.id).unwrap_or(&no_verdict);
        VerdictRow {
            verdict: verdict_of(&verdict.verdict),
            text: item.text.clone(),
            reason: verdict.reason.clone(),
            evidence: verdict.evidence.clone(),
            decided_by: decided_by(verdict),
            alternatives: item
                .alternatives
                .iter()
                .flatten()
                .map(|alternative| Self::from(alternative, verdicts))
                .collect(),
        }
    }

    /// A saved match's item (cv_match_items).
    pub fn from_saved(item: &SavedItem) -> Self {
        VerdictRow {
            verdict: verdict_of(&item.verdict),
            text: item.text.clone(),
            reason: item.reason.clone(),
            evidence: item.evidence.clone(),
            decided_by: saved_decided_by(&item.decided_by),
            alternatives: Vec::new(),
        }
    }
}

/// The rows sorted missing first; rows with the same verdict keep their order.
pub fn sorted(mut rows: Vec<VerdictRow>) -> Vec<VerdictRow> {
    rows.sort_by_key(|row| order(row.verdict));
    rows
}

fn order(verdict: &str) -> usize {
    ORDER.iter().position(|v| *v == verdict).unwrap_or(0)
}

/// The verdict as one of ORDER; anything else counts as missing.
fn verdict_of(verdict: &str) -> &'static str {
    ORDER.iter().find(|v| **v == verdict).copied().unwrap_or("missing")
}

fn decided_by(verdict: &Verdict) -> String {
    match (verdict.by.as_str(), verdict.method.as_str()) {
        ("llm", _) => "model".to_string(),
        ("rule", _) => "rule".to_string(),
        (_, "not_found") => "keyword match".to_string(),
        (_, method) => format!("taxonomy, {method}"),
    }
}

/// The same words for a saved item, where decided_by is the method ("llm" and "years" aside).
fn saved_decided_by(decided_by: &str) -> String {
    match decided_by {
        "llm" => "model".to_string(),
        "years" => "rule".to_string(),
        "not_found" => "keyword match".to_string(),
        method => format!("taxonomy, {method}"),
    }
}
