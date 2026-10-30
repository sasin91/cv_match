//! A match as the page prints its result (templates/cv_match/result.html), ported from
//! modules/cv_match/Match_result.php: the score in its ring, the job, how the score is made up,
//! a location the candidate likely misses, what the post asks of applicants, the Reject button
//! of a saved match, the requirements by group with their verdicts and a filter by verdict, the
//! soft skills, Laya's reading and, for a saved match, the application and résumé panel.
//!
//! from() takes a match just made (the free match); from_saved() a saved one, which keeps less:
//! no part weights, no alternatives, no soft skills.

use indexmap::IndexMap;

use super::apply_instructions::ApplyInstructions;
use super::apply_notice::ApplyNotice;
use super::cv_match_model::SavedMatch;
use super::cv_matcher::{Groups, Part, Score, Verdicts};
use super::location_notice::LocationNotice;
use super::match_documents::MatchDocuments;
use super::match_score::MatchScore;
use super::reject_button::RejectButton;
use super::verdict_row::{self, VerdictRow};
use super::{Job, Profile, dates};

/// The groups' names, in the order a saved match's unknown criterion falls back from.
pub const GROUP_NAMES: [(&str, &str); 5] = [
    ("requirements", "Requirements"),
    ("skills", "Nice to have"),
    ("title", "Job title"),
    ("responsibilities", "Responsibilities"),
    ("location", "Location"),
];
pub const LOCATION_NOTE: &str = "not weighed · a miss caps the score";
pub const FILTERS: [(&str, &str); 3] = [("missing", "Missing"), ("partial", "Partial"), ("met", "Met")];
pub const ALL: &str = "All";
/// Most CV terms the CV panel shows as chips.
const CHIPS_SHOWN: usize = 30;

/// One group of requirements with its rows, missing first.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub heading: String,
    pub note: String,
    pub rows: Vec<VerdictRow>,
}

/// A filter button: All, then each verdict there is, with how many.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    pub key: &'static str,
    pub label: &'static str,
    pub count: usize,
}

/// Laya's answer as a line; no text without Laya.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayaLine {
    pub text: String,
    pub failed: bool,
}

/// What the free match found in the CV, for the CV panel.
#[derive(Debug, Clone, PartialEq)]
pub struct CvFound {
    /// "Found 6 years of experience, 12 skills and 2 job titles we know."
    pub status: String,
    pub terms: Vec<String>,
    /// "and 4 more", or ''.
    pub more: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub saved_id: Option<i64>,
    pub score: MatchScore,
    pub heading: String,
    pub parts: String,
    pub saved_at: String,
    pub reject: Option<RejectButton>,
    pub location: Option<LocationNotice>,
    pub apply: Option<ApplyNotice>,
    pub filters: Vec<Filter>,
    pub sections: Vec<Section>,
    pub soft_skills: Vec<String>,
    pub laya: LayaLine,
    pub laya_input: String,
    pub documents: Option<MatchDocuments>,
    pub job_url: String,
    pub job_text: String,
    pub cv_found: Option<CvFound>,
}

/// A match just made: the job post and CV as read, its groups, combined verdicts and score.
pub struct Fresh<'a> {
    pub job: &'a Job,
    pub groups: &'a Groups,
    pub verdicts: &'a Verdicts,
    pub score: &'a Score,
    pub soft_skills: Vec<String>,
    pub apply_instructions: Option<&'a ApplyInstructions>,
    pub saved_id: Option<i64>,
}

impl MatchResult {
    /// writer: the page can write the application; documents: None for the free match.
    pub fn from(fresh: &Fresh, writer: bool, documents: Option<MatchDocuments>, laya_input: &str) -> Self {
        let sections = sections(fresh.groups, fresh.verdicts, &fresh.score.parts);
        let title = if fresh.job.job_title.is_empty() {
            &fresh.job.heading
        } else {
            &fresh.job.job_title
        };
        MatchResult {
            saved_id: fresh.saved_id,
            score: MatchScore::of(fresh.score.index, &fresh.score.tag),
            heading: heading(title, &fresh.job.company),
            parts: parts(&fresh.score.parts),
            saved_at: String::new(),
            reject: RejectButton::from(fresh.saved_id, None),
            location: LocationNotice::from(fresh.groups, fresh.verdicts),
            apply: ApplyNotice::from(fresh.apply_instructions, writer),
            filters: filters(&sections),
            sections,
            soft_skills: fresh.soft_skills.clone(),
            laya: LayaLine::default(),
            laya_input: laya_input.to_string(),
            documents,
            job_url: String::new(),
            job_text: String::new(),
            cv_found: None,
        }
    }

    /// A saved match with its items; the page puts its post back in the job box.
    pub fn from_saved(found: &SavedMatch, writer: bool, documents: MatchDocuments) -> Self {
        let sections = saved_sections(found);
        MatchResult {
            saved_id: Some(found.id),
            score: MatchScore::of(found.score, &found.tag),
            heading: heading(&found.job_title, &found.company),
            parts: format!("{}/{} points", found.points, found.max_points),
            saved_at: dates::iso(found.created_at),
            reject: RejectButton::from(Some(found.id), found.rejected_at.map(|_| found.rejections.as_slice())),
            location: LocationNotice::from_saved(&found.items),
            apply: ApplyNotice::from(found.apply_instructions.as_ref(), writer),
            filters: filters(&sections),
            sections,
            soft_skills: Vec::new(),
            laya: LayaLine::default(),
            laya_input: String::new(),
            documents: Some(documents),
            job_url: found.job_url.clone().unwrap_or_default(),
            job_text: found.job_text.clone(),
            cv_found: None,
        }
    }

    /// The result with what the free match found in the CV.
    pub fn with_cv_found(mut self, profile: &Profile) -> Self {
        self.cv_found = Some(cv_found(profile));
        self
    }
}

/// What the free match found in the CV, for the CV panel.
pub fn cv_found(profile: &Profile) -> CvFound {
    let found: Vec<String> = profile.title.iter().chain(&profile.skill).cloned().collect();
    let years = if profile.experience_years != 0 {
        format!("{} years of experience, ", profile.experience_years)
    } else {
        String::new()
    };
    CvFound {
        status: format!(
            "Found {years}{} skills and {} job titles we know.",
            profile.skill.len(),
            profile.title.len()
        ),
        more: if found.len() > CHIPS_SHOWN {
            format!("and {} more", found.len() - CHIPS_SHOWN)
        } else {
            String::new()
        },
        terms: found.into_iter().take(CHIPS_SHOWN).collect(),
    }
}

fn group_name(key: &str) -> Option<&'static str> {
    GROUP_NAMES.iter().find(|(k, _)| *k == key).map(|(_, name)| *name)
}

/// "Job title · Company".
fn heading(title: &str, company: &str) -> String {
    [title, company]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// "Requirements 80% (120/150) · Nice to have 50% (25/50) · ...".
fn parts(parts: &IndexMap<String, Part>) -> String {
    parts
        .iter()
        .map(|(key, part)| {
            format!(
                "{} {}% ({}/{})",
                group_name(key).unwrap_or(key),
                part.percentage,
                part.value,
                part.weight
            )
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The groups with requirements, missing first.
fn sections(groups: &Groups, verdicts: &Verdicts, parts: &IndexMap<String, Part>) -> Vec<Section> {
    groups
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(key, items)| Section {
            heading: group_name(key).unwrap_or(key).to_string(),
            note: match parts.get(key) {
                Some(part) => format!("{}% · weight {}", part.percentage, part.weight),
                None => LOCATION_NOTE.to_string(),
            },
            rows: verdict_row::sorted(items.iter().map(|item| VerdictRow::from(item, verdicts)).collect()),
        })
        .collect()
}

/// A saved match's items by criterion, in the order they come, missing first.
fn saved_sections(found: &SavedMatch) -> Vec<Section> {
    let mut by_criterion: IndexMap<&str, Vec<VerdictRow>> = IndexMap::new();
    for item in &found.items {
        by_criterion
            .entry(item.criterion.as_str())
            .or_default()
            .push(VerdictRow::from_saved(item));
    }
    by_criterion
        .into_iter()
        .map(|(criterion, rows)| Section {
            heading: group_name(criterion).unwrap_or(criterion).to_string(),
            note: String::new(),
            rows: verdict_row::sorted(rows),
        })
        .collect()
}

fn filters(sections: &[Section]) -> Vec<Filter> {
    let verdicts: Vec<&str> = sections
        .iter()
        .flat_map(|section| section.rows.iter().map(|row| row.verdict))
        .collect();
    if verdicts.is_empty() {
        return Vec::new();
    }
    let mut filters = vec![Filter {
        key: "all",
        label: ALL,
        count: verdicts.len(),
    }];
    for (key, label) in FILTERS {
        let count = verdicts.iter().filter(|verdict| **verdict == key).count();
        if count > 0 {
            filters.push(Filter { key, label, count });
        }
    }
    filters
}
