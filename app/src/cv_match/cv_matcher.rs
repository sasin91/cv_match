//! Matching a job post against a CV, and the legacy ranking of the result, ported from
//! modules/cv_match/Cv_matcher.php.
//!
//! A job post is the extract_job answer and a profile the extract_cv answer (see `Job` and
//! `Profile`). Verdicts are keyed by item id: { verdict: met|partial|missing, by, method,
//! evidence, reason }.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{Job, KnownTerm, Profile};
use crate::php;
use crate::taxonomy::{self, Method, Taxonomy, Term};

/// The legacy ranking engine's weights: formal requirements weigh 150, relevant skills 50, and
/// every other criterion (job title, responsibility area) 30.
pub mod weights {
    pub const REQUIREMENTS: i64 = 150;
    pub const SKILLS: i64 = 50;
    pub const TITLE: i64 = 30;
    pub const RESPONSIBILITIES: i64 = 30;
}

/// How much a verdict counts towards a criterion. The legacy ranker only knew in or out; a
/// partial match counts half.
pub mod credit {
    pub const MET: f64 = 1.0;
    pub const PARTIAL: f64 = 0.5;
    pub const MISSING: f64 = 0.0;
}

/// The most a match may score when the post asks the candidate to live somewhere they don't (a
/// missing required location): just under "medium", so it ranks below real fits and isn't
/// tailored right away.
pub const LOCATION_MISSED_INDEX: f64 = 0.59;

/// The legacy ranker's tags, by the lowest match index that earns them.
pub mod tags {
    pub const TOP_FROM: f64 = 0.85;
    pub const GOOD_FROM: f64 = 0.8;
    pub const MEDIUM_FROM: f64 = 0.6;
}

/// Taxonomy match methods precise enough to decide a requirement on their own. An
/// embedding-only hit is a guess, so those go to the model.
const DECISIVE: [Method; 3] = [Method::Exact, Method::Synonym, Method::Fuzzy];

/// How many ranked terms map_term() looks through.
const MAP_TERM_CANDIDATES: usize = 3;

/// Most edits two spellings of a requirement may differ by ("Kubernets").
const SPELLING_EDITS: usize = 2;

/// The criteria, in the order the score adds them up.
pub const REQUIREMENTS: &str = "requirements";
pub const SKILLS: &str = "skills";
pub const TITLE: &str = "title";
pub const RESPONSIBILITIES: &str = "responsibilities";
pub const LOCATION: &str = "location";

/// One thing a verdict is given on, or a group of alternatives met by the best of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub english: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_years: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<Vec<Item>>,
}

/// The job post's items by criterion: requirements, skills, title, responsibilities and, when
/// the post has any required location, location.
pub type Groups = IndexMap<String, Vec<Item>>;

/// A verdict on one item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// met, partial or missing.
    pub verdict: String,
    /// Who decided: rule, taxonomy or llm.
    pub by: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub reason: String,
}

pub type Verdicts = IndexMap<String, Verdict>;

/// One criterion's share of the score.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Part {
    pub percentage: i64,
    pub value: i64,
    pub weight: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    pub rank: i64,
    pub total: i64,
    pub index: f64,
    pub tag: String,
    pub parts: IndexMap<String, Part>,
}

/// What a CV phrase names in the taxonomy.
struct CvTerm {
    index: usize,
    phrase: String,
}

pub struct CvMatcher<'a> {
    taxonomy: &'a Taxonomy,
    profile: Profile,
    known: IndexMap<String, KnownTerm>,
    cv_phrases: Vec<String>,
    /// By term id, in the order the CV names them.
    cv_terms: IndexMap<i64, CvTerm>,
}

impl<'a> CvMatcher<'a> {
    /// `known` holds phrases whose term is already known, phrase => term (with its match method),
    /// so map_term() needn't rank the taxonomy for them: Free_reader found the CV's and job
    /// post's phrases by label.
    pub fn new(taxonomy: &'a Taxonomy, profile: Profile, known: IndexMap<String, KnownTerm>) -> Self {
        let cv_phrases: Vec<String> = [
            &profile.skill,
            &profile.language,
            &profile.certificate,
            &profile.education,
            &profile.title,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        let mut matcher = Self {
            taxonomy,
            profile,
            known,
            cv_phrases,
            cv_terms: IndexMap::new(),
        };
        for phrase in matcher.cv_phrases.clone() {
            if let Some(found) = matcher.map_term(&phrase, None) {
                let id = taxonomy.terms[found.index].id;
                matcher.cv_terms.entry(id).or_insert(CvTerm {
                    index: found.index,
                    phrase,
                });
            }
        }
        matcher
    }

    /// The taxonomy term a phrase names, when the lookup is sure: the phrase is a label of the
    /// term, a synonym, or one with typos. None otherwise.
    pub fn map_term(&self, phrase: &str, kind: Option<&str>) -> Option<KnownTerm> {
        if let Some(&known) = self.known.get(phrase)
            && kind.is_none_or(|k| self.taxonomy.terms[known.index].kind == k)
        {
            return Some(known);
        }
        for result in self.taxonomy.rank(phrase, kind, MAP_TERM_CANDIDATES) {
            let method = self.taxonomy.match_method(&result, phrase);
            if DECISIVE.contains(&method) {
                let index = self.taxonomy.terms.iter().position(|t| std::ptr::eq(t, result.term))?;
                return Some(KnownTerm { index, method });
            }
        }
        None
    }

    /// A verdict the taxonomy (or plain arithmetic) can give without the model, or None when it
    /// can't decide.
    pub fn decide(&self, item: &Item) -> Option<Verdict> {
        let min_years = item.min_years.unwrap_or(0);
        if item.kind == "experience" && min_years > 0 {
            let have = self.profile.experience_years;
            let verdict = if have >= min_years {
                "met"
            } else if have as f64 >= min_years as f64 / 2.0 {
                "partial"
            } else {
                "missing"
            };
            return Some(verdict_of(
                verdict,
                "rule",
                "years",
                format!("{have} years"),
                format!("Needs {min_years}, CV shows {have}."),
            ));
        }
        // A level ("fluent", "master's degree") is more than the taxonomy knows, and where
        // someone lives is the model's to weigh.
        if matches!(item.kind.as_str(), "location" | "responsibility" | "experience")
            || item.min_level.as_deref().is_some_and(|l| !l.is_empty())
        {
            return None;
        }

        // Spelled the same, give or take a typo ("Kubernets").
        let wanted = self.taxonomy.content_words(&item.text).join(" ");
        for phrase in &self.cv_phrases {
            let have = self.taxonomy.content_words(phrase).join(" ");
            if wanted.is_empty() || have.is_empty() {
                continue;
            }
            let distance = taxonomy::edit_distance(&wanted, &have, SPELLING_EDITS);
            if distance <= taxonomy::typo_budget(&wanted) {
                let method = if distance == 0 { "exact" } else { "fuzzy" };
                return Some(verdict_of(
                    "met",
                    "taxonomy",
                    method,
                    phrase.clone(),
                    "Named in the CV.".into(),
                ));
            }
        }

        let found = self.map_term(&item.text, None)?;
        let term = &self.taxonomy.terms[found.index];
        let label = term.preferred_da();

        if let Some(same) = self.cv_terms.get(&term.id) {
            return Some(verdict_of(
                "met",
                "taxonomy",
                found.method.as_str(),
                same.phrase.clone(),
                format!("Both are \"{label}\"."),
            ));
        }
        // A broader or narrower skill in the same family, or a sibling.
        for cv_term in self.cv_terms.values() {
            let have = &self.taxonomy.terms[cv_term.index];
            if related(term, have) {
                return Some(verdict_of(
                    "partial",
                    "taxonomy",
                    "related",
                    cv_term.phrase.clone(),
                    format!("\"{}\" is related to \"{label}\".", have.preferred_da()),
                ));
            }
        }
        None
    }
}

/// Whether `have` is the parent, a child or a sibling of `term`.
fn related(term: &Term, have: &Term) -> bool {
    let parent = term.parent_id.filter(|&p| p != 0);
    let have_parent = have.parent_id.filter(|&p| p != 0);
    parent.is_some_and(|p| p == have.id || Some(p) == have.parent_id) || have_parent == Some(term.id)
}

fn verdict_of(verdict: &str, by: &str, method: &str, evidence: String, reason: String) -> Verdict {
    Verdict {
        verdict: verdict.into(),
        by: by.into(),
        method: method.into(),
        evidence,
        reason,
    }
}

/// The verdict a stored credit (CREDIT, as match_score_details.rank) stands for.
pub fn verdict_of_credit(value: f64) -> &'static str {
    if value >= credit::MET {
        "met"
    } else if value > credit::MISSING {
        "partial"
    } else {
        "missing"
    }
}

/// The legacy ranker's tag for a match index.
pub fn match_tag(index: f64) -> &'static str {
    if index >= tags::TOP_FROM {
        "top"
    } else if index >= tags::GOOD_FROM {
        "good"
    } else if index >= tags::MEDIUM_FROM {
        "medium"
    } else {
        "poor"
    }
}

fn credit_of(verdict: Option<&Verdict>) -> f64 {
    match verdict.map(|v| v.verdict.as_str()) {
        Some("met") => credit::MET,
        Some("partial") => credit::PARTIAL,
        _ => credit::MISSING,
    }
}

fn weight_of(criterion: &str) -> i64 {
    match criterion {
        REQUIREMENTS => weights::REQUIREMENTS,
        SKILLS => weights::SKILLS,
        TITLE => weights::TITLE,
        _ => weights::RESPONSIBILITIES,
    }
}

/// The job post as the legacy ranker's criteria: one list of items per weight. The post's one
/// requirements list splits into required (the formal requirements) and nice-to-have (relevant
/// skills). Requirements sharing an alt_group are alternatives and make one item, met by its
/// best alternative.
///
/// Soft skills are left out: they are not scored (see soft_skills()). Required location
/// requirements are their own group, location, when the post has any: not weighed, but a missing
/// one caps the score. A nice-to-have location is a relevant skill like any other.
pub fn criteria(job: &Job) -> Groups {
    let mut lists: IndexMap<&str, Vec<Item>> =
        IndexMap::from([(REQUIREMENTS, vec![]), (SKILLS, vec![]), (LOCATION, vec![])]);
    // "list:alt_group" => (list, position)
    let mut groups: IndexMap<String, (&str, usize)> = IndexMap::new();
    for (i, r) in job.requirements.iter().enumerate() {
        if r.kind == "soft_skill" {
            continue;
        }
        let level = if r.min_level.is_empty() {
            String::new()
        } else {
            format!(" ({})", r.min_level)
        };
        let english = if r.english.is_empty() { &r.value } else { &r.english };
        let unit = Item {
            id: format!("r{i}"),
            kind: r.kind.clone(),
            text: format!("{}{level}", r.value),
            english: Some(format!("{english}{level}")),
            min_years: Some(r.min_years),
            min_level: Some(r.min_level.clone()),
            alternatives: None,
        };
        let list = if !r.required {
            SKILLS
        } else if r.kind == "location" {
            LOCATION
        } else {
            REQUIREMENTS
        };
        if r.alt_group.is_empty() {
            lists[list].push(unit);
            continue;
        }
        let key = format!("{list}:{}", r.alt_group);
        let (list, position) = match groups.get(&key) {
            Some(&found) => found,
            None => {
                let position = lists[list].len();
                lists[list].push(Item {
                    id: format!("g{}", groups.len()),
                    kind: "alternatives".into(),
                    text: String::new(),
                    english: None,
                    min_years: None,
                    min_level: None,
                    alternatives: Some(vec![]),
                });
                groups.insert(key, (list, position));
                (list, position)
            }
        };
        if let Some(alternatives) = lists[list][position].alternatives.as_mut() {
            alternatives.push(unit);
        }
    }
    for &(list, position) in groups.values() {
        let item = &mut lists[list][position];
        let alternatives = item.alternatives.as_deref().unwrap_or_default();
        let text = alternatives
            .iter()
            .map(|a| a.text.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let english = alternatives
            .iter()
            .map(|a| a.english.as_deref().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" or ");
        item.text = text;
        item.english = Some(english);
    }

    let responsibilities = job
        .responsibilities
        .iter()
        .enumerate()
        .map(|(i, text)| Item {
            id: format!("a{i}"),
            kind: "responsibility".into(),
            text: text.clone(),
            english: None,
            min_years: None,
            min_level: None,
            alternatives: None,
        })
        .collect();
    let title = if job.job_title.is_empty() {
        vec![]
    } else {
        vec![Item {
            id: "t0".into(),
            kind: "title".into(),
            text: job.job_title.clone(),
            english: Some(job.job_title_en.clone()),
            min_years: None,
            min_level: None,
            alternatives: None,
        }]
    };
    let location = lists.swap_remove(LOCATION).unwrap_or_default();
    let mut out = Groups::new();
    out.insert(REQUIREMENTS.into(), lists.swap_remove(REQUIREMENTS).unwrap_or_default());
    out.insert(SKILLS.into(), lists.swap_remove(SKILLS).unwrap_or_default());
    out.insert(TITLE.into(), title);
    out.insert(RESPONSIBILITIES.into(), responsibilities);
    if !location.is_empty() {
        out.insert(LOCATION.into(), location);
    }
    out
}

/// The post's location items the model judged the candidate misses: what caps the score. Only
/// the model weighs where someone lives, so a location nobody judged (no AI key) never caps it.
pub fn missed_locations<'g>(groups: &'g Groups, verdicts: &Verdicts) -> Vec<&'g Item> {
    groups
        .get(LOCATION)
        .into_iter()
        .flatten()
        .filter(|item| {
            verdicts
                .get(&item.id)
                .is_some_and(|v| v.verdict == "missing" && v.by == "llm")
        })
        .collect()
}

/// The post's soft skills, shown as tags but not scored.
pub fn soft_skills(job: &Job) -> Vec<String> {
    job.requirements
        .iter()
        .filter(|r| r.kind == "soft_skill")
        .map(|r| r.value.clone())
        .collect()
}

/// What gets a verdict: every item, or each of its alternatives.
pub fn units(groups: &Groups) -> Vec<&Item> {
    groups
        .values()
        .flatten()
        .flat_map(|item| match &item.alternatives {
            Some(alternatives) => alternatives.iter().collect::<Vec<_>>(),
            None => vec![item],
        })
        .collect()
}

/// Verdicts on units, plus one per alternatives item: its best alternative.
pub fn combine(groups: &Groups, verdicts: &Verdicts) -> Verdicts {
    let rank = |v: &Verdict| match v.verdict.as_str() {
        "met" => 2,
        "partial" => 1,
        _ => 0,
    };
    let mut out = verdicts.clone();
    for item in groups.values().flatten() {
        let Some(alternatives) = &item.alternatives else {
            continue;
        };
        let mut top: Option<&Verdict> = None;
        for a in alternatives {
            if let Some(v) = verdicts.get(&a.id)
                && top.is_none_or(|t| rank(v) > rank(t))
            {
                top = Some(v);
            }
        }
        if let Some(top) = top {
            out.insert(item.id.clone(), top.clone());
        }
    }
    out
}

/// The verdicts on the requirements as one English sentence pair, the state Laya's English
/// checkpoint ranked well in the bake-off (eval/laya/run_laya_summary.py). A partial match counts
/// as missing there, marked "(partly)".
pub fn laya_summary(job: &Job, groups: &Groups, verdicts: &Verdicts) -> String {
    // A required location is a required qualification to Laya too, once the model has weighed
    // it: unjudged, it isn't known to be missing.
    let judged = groups
        .get(LOCATION)
        .into_iter()
        .flatten()
        .filter(|item| verdicts.get(&item.id).is_some_and(|v| v.by == "llm"));
    let items: Vec<&Item> = groups.get(REQUIREMENTS).into_iter().flatten().chain(judged).collect();
    let mut met = 0;
    let mut missing = Vec::new();
    for item in &items {
        let verdict = verdicts.get(&item.id).map(|v| v.verdict.as_str());
        let name = item.english.as_deref().filter(|e| !e.is_empty()).unwrap_or(&item.text);
        match verdict {
            Some("met") => met += 1,
            Some("partial") => missing.push(format!("{name} (partly)")),
            _ => missing.push(name.to_string()),
        }
    }
    let title = if job.job_title_en.is_empty() {
        &job.job_title
    } else {
        &job.job_title_en
    };
    let missing = if missing.is_empty() {
        "none".to_string()
    } else {
        missing.join(", ")
    };
    format!(
        "Job: {title}. The candidate meets {met} of {} required qualifications. Missing requirements: {missing}.",
        items.len()
    )
}

/// The legacy ranking over the criteria: each non-empty criterion scores the share of its items
/// met (as a whole percent), times its weight; the match index is the sum over the sum of
/// weights. A missed required location caps the index at LOCATION_MISSED_INDEX.
pub fn score(groups: &Groups, verdicts: &Verdicts) -> Score {
    /// A share as a whole percent.
    const PERCENT: f64 = 100.0;
    let mut rank = 0;
    let mut total = 0;
    let mut parts = IndexMap::new();
    for (key, items) in groups {
        if items.is_empty() || key == LOCATION {
            continue;
        }
        let weight = weight_of(key);
        let credit: f64 = items.iter().map(|item| credit_of(verdicts.get(&item.id))).sum();
        let percentage = (credit / items.len() as f64 * PERCENT) as i64;
        let value = php::round(percentage as f64 / PERCENT * weight as f64) as i64;
        rank += value;
        total += weight;
        parts.insert(
            key.clone(),
            Part {
                percentage,
                value,
                weight,
            },
        );
    }
    let mut index = if total > 0 { rank as f64 / total as f64 } else { 0.0 };
    if !missed_locations(groups, verdicts).is_empty() {
        index = index.min(LOCATION_MISSED_INDEX);
    }
    Score {
        rank,
        total,
        index,
        tag: match_tag(index).into(),
        parts,
    }
}
