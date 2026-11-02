//! What an AI match teaches the taxonomy (taxonomy_store::learn), so the same requirement is
//! found by rule next time, with no model call:
//!
//! - From the model's read of the post: a requirement the taxonomy doesn't know by its own words,
//!   whose English does name a term by one of its labels ("Erfaring med C#" → C#).
//! - From the judge's "met": a requirement the taxonomy doesn't know, whose evidence is really in
//!   the CV and holds exactly one of the CV's own list items that names a term ("datamatiker" →
//!   the term the CV's "Datamatikeruddannelse" names). "partial" and "missing" teach nothing.
//!
//! Locations and years of experience are never learned: the judge weighs those against the
//! candidate, not against a term. Short phrases, and phrases of only stopwords and qualifiers
//! ("erfaring med"), are skipped.

use indexmap::IndexSet;

use super::matcher::{Matcher, Verdicts};
use super::{Job, KnownTerm, Profile, Requirement};
use crate::taxonomy::{Method, Taxonomy};
use crate::taxonomy_store::{FROM_JUDGE, FROM_READ, Lesson};

/// The shortest phrase worth learning, in characters.
const MIN_PHRASE_CHARS: usize = 3;

/// Requirement kinds that are never learned.
const NOT_LEARNED: [&str; 2] = ["location", "experience"];

/// A requirement's unit id in the matcher's criteria ("r3" for the fourth requirement).
const UNIT_PREFIX: &str = "r";

/// The verdict that teaches.
const MET: &str = "met";

/// Who decided a verdict when the model judged it.
const BY_LLM: &str = "llm";

/// Words that mark a phrase as Danish, besides æ, ø and å.
const DANISH_WORDS: [&str; 12] = [
    "og",
    "med",
    "til",
    "af",
    "på",
    "i",
    "erfaring",
    "kendskab",
    "uddannelse",
    "evne",
    "god",
    "gode",
];

/// What job, read by the model and matched with profile, teaches the taxonomy. cv_text is the CV
/// the judge saw; verdicts are the match's.
pub fn lessons(taxonomy: &Taxonomy, job: &Job, profile: &Profile, cv_text: &str, verdicts: &Verdicts) -> Vec<Lesson> {
    let matcher = Matcher::new(taxonomy, profile.clone(), Default::default());
    let mut lessons: IndexSet<Lesson> = IndexSet::new();
    for requirement in learnable_requirements(taxonomy, &matcher, job) {
        if let Some(term_id) = english_term(taxonomy, &matcher, requirement) {
            lessons.insert(lesson(requirement, term_id, FROM_READ));
        }
    }
    let cv = normalized(cv_text);
    for (id, verdict) in verdicts {
        if verdict.by != BY_LLM || verdict.verdict != MET || verdict.evidence.trim().is_empty() {
            continue;
        }
        let Some(requirement) = requirement_of(job, id) else {
            continue;
        };
        if !is_learnable(taxonomy, &matcher, requirement) {
            continue;
        }
        let evidence = normalized(&verdict.evidence);
        if !cv.contains(&evidence) {
            continue;
        }
        if let Some(term_id) = cv_item_term(taxonomy, &matcher, profile, &evidence) {
            lessons.insert(lesson(requirement, term_id, FROM_JUDGE));
        }
    }
    lessons.into_iter().collect()
}

fn lesson(requirement: &Requirement, term_id: i64, learned_from: &'static str) -> Lesson {
    let text = requirement.value.trim().to_string();
    Lesson {
        term_id,
        lang: phrase_lang(&text),
        text,
        learned_from,
    }
}

/// The post's requirements worth learning from.
fn learnable_requirements<'a>(
    taxonomy: &'a Taxonomy,
    matcher: &'a Matcher<'a>,
    job: &'a Job,
) -> impl Iterator<Item = &'a Requirement> {
    job.requirements
        .iter()
        .filter(move |requirement| is_learnable(taxonomy, matcher, requirement))
}

/// Whether a requirement can teach: not a location or years, long enough, with words that say
/// something, and not already known to the taxonomy by its own words.
fn is_learnable(taxonomy: &Taxonomy, matcher: &Matcher<'_>, requirement: &Requirement) -> bool {
    let value = requirement.value.trim();
    !NOT_LEARNED.contains(&requirement.kind.as_str())
        && value.chars().count() >= MIN_PHRASE_CHARS
        && !taxonomy.content_words(value).is_empty()
        && matcher.map_term(value, None).is_none()
}

/// The term the requirement's English names by a label (not by a typo), when it differs from the
/// requirement's own words.
fn english_term(taxonomy: &Taxonomy, matcher: &Matcher<'_>, requirement: &Requirement) -> Option<i64> {
    let english = requirement.english.trim();
    if english.is_empty() || english.eq_ignore_ascii_case(requirement.value.trim()) {
        return None;
    }
    by_label(taxonomy, matcher.map_term(english, None)?)
}

/// The one term the CV's own list items in evidence name by a label; None when none or several do.
fn cv_item_term(taxonomy: &Taxonomy, matcher: &Matcher<'_>, profile: &Profile, evidence: &str) -> Option<i64> {
    let items = [&profile.skill, &profile.title, &profile.education];
    let terms: IndexSet<i64> = items
        .into_iter()
        .flatten()
        .filter(|item| {
            let item = normalized(item);
            !item.is_empty() && evidence.contains(&item)
        })
        .filter_map(|item| by_label(taxonomy, matcher.map_term(item, None)?))
        .collect();
    match terms.len() {
        1 => terms.first().copied(),
        _ => None,
    }
}

/// A mapped term's id, when the phrase is one of its labels (not a guess with typos).
fn by_label(taxonomy: &Taxonomy, known: KnownTerm) -> Option<i64> {
    matches!(known.method, Method::Exact | Method::Synonym).then(|| taxonomy.terms[known.index].id)
}

/// The requirement a verdict's unit id ("r3") stands for.
fn requirement_of<'a>(job: &'a Job, unit_id: &str) -> Option<&'a Requirement> {
    let index: usize = unit_id.strip_prefix(UNIT_PREFIX)?.parse().ok()?;
    job.requirements.get(index)
}

/// Text lowercased with its whitespace made single spaces, for finding a quote in the CV.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// "da" for a phrase with æ, ø or å or a Danish word in it, else "en".
fn phrase_lang(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    let danish_letters = lower.chars().any(|c| matches!(c, 'æ' | 'ø' | 'å'));
    let danish_word = lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| DANISH_WORDS.contains(&word));
    if danish_letters || danish_word { "da" } else { "en" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matches::matcher::Verdict;

    fn requirement(value: &str, english: &str, kind: &str) -> Requirement {
        Requirement {
            value: value.to_string(),
            english: english.to_string(),
            kind: kind.to_string(),
            required: true,
            ..Default::default()
        }
    }

    /// A term's preferred label in lang, and the term's id.
    fn preferred(taxonomy: &Taxonomy, kind: &str, lang: &str) -> (String, i64) {
        taxonomy
            .terms
            .iter()
            .filter(|term| term.kind == kind)
            .find_map(|term| {
                let label = term.labels.iter().find(|l| l.preferred && l.lang == lang)?;
                (label.text.chars().count() > MIN_PHRASE_CHARS + 2).then(|| (label.text.clone(), term.id))
            })
            .unwrap()
    }

    #[test]
    fn a_requirement_whose_english_names_a_term_is_learned_in_its_own_words() {
        let taxonomy = Taxonomy::load();
        let (english, term_id) = preferred(&taxonomy, "skill", "en");
        let job = Job {
            requirements: vec![
                requirement("Qzxv på højt niveau", &english, "skill"),
                requirement("Bosat på Sjælland", &english, "location"),
            ],
            ..Default::default()
        };
        let found = lessons(&taxonomy, &job, &Profile::default(), "", &Verdicts::new());
        assert_eq!(
            found,
            [Lesson {
                term_id,
                text: "Qzxv på højt niveau".to_string(),
                lang: "da",
                learned_from: FROM_READ,
            }]
        );
    }

    #[test]
    fn a_met_verdict_teaches_only_with_a_real_quote_of_a_cv_item() {
        let taxonomy = Taxonomy::load();
        let (item, term_id) = preferred(&taxonomy, "skill", "da");
        let profile = Profile {
            skill: vec![item.clone()],
            ..Default::default()
        };
        let cv_text = format!("Erfaring:\n  Arbejdede med {item} i fem år.");
        let job = Job {
            requirements: vec![requirement("Qzxv på højt niveau", "", "skill")],
            ..Default::default()
        };
        let met = |evidence: &str| {
            Verdicts::from([(
                "r0".to_string(),
                Verdict {
                    verdict: MET.to_string(),
                    by: BY_LLM.to_string(),
                    method: String::new(),
                    evidence: evidence.to_string(),
                    reason: String::new(),
                },
            )])
        };
        let quoted = format!("arbejdede med {item}");
        assert_eq!(
            lessons(&taxonomy, &job, &profile, &cv_text, &met(&quoted)),
            [Lesson {
                term_id,
                text: "Qzxv på højt niveau".to_string(),
                lang: "da",
                learned_from: FROM_JUDGE,
            }]
        );
        let made_up = format!("ledte et team med {item}");
        assert!(lessons(&taxonomy, &job, &profile, &cv_text, &met(&made_up)).is_empty());
    }

    #[test]
    fn a_phrase_is_danish_by_its_letters_or_words() {
        assert_eq!(phrase_lang("Kørekort B"), "da");
        assert_eq!(phrase_lang("Erfaring med C#"), "da");
        assert_eq!(phrase_lang("Experience with C#"), "en");
    }
}
