//! One job post through the match, for the page's match endpoint (and the job scan to come): the
//! requirements decided by the taxonomy against the CV, the rest judged by the model (or, without
//! one, left missing as the free match leaves them), then scored and saved for the user. The job
//! post and CV come read by match_prompts or by the free reader. Never a URL.

use indexmap::IndexMap;
use serde::Serialize;
use sqlx::SqlitePool;

use super::apply_instructions::{self, ApplyInstructions};
use super::match_model::{self, NewMatch};
use super::match_prompts::{self, Undecided};
use super::matcher::{self, Groups, LOCATION, Matcher, Score, Verdict, Verdicts};
use super::{Job, KnownTerm, Profile};
use crate::laya::laya_client::{self, LayaClient, LayaReading};
use crate::llm::{Llm, LlmException};
use crate::taxonomy::Taxonomy;

/// What a requirement the taxonomy didn't find in the CV says, when no model looked further.
const NOT_FOUND_REASON: &str =
    "Not named in the CV. With an AI key on for evaluating job posts, the AI also looks for related experience.";

/// What a location requirement nobody weighed says.
const UNWEIGHED_LOCATION_REASON: &str =
    "Check this yourself: with an AI key on for evaluating job posts, the AI weighs it against where you live.";

/// The match's answer: the criteria, the combined verdicts and the score, with the soft skills,
/// how the post asks to be applied to, the Laya summary and, with Laya's model, its answer on that
/// summary (laya), or laya_error; saved_id once saved, or save_error when the database couldn't
/// take it (the score still comes back).
#[derive(Debug, Clone, Serialize)]
pub struct AiMatch {
    pub groups: Groups,
    pub verdicts: Verdicts,
    pub result: Score,
    pub soft_skills: Vec<String>,
    pub apply_instructions: Option<ApplyInstructions>,
    pub laya_summary: String,
    /// laya and laya_error.
    #[serde(flatten)]
    pub laya_reading: LayaReading,
    pub saved_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_error: Option<String>,
}

/// What the match needs besides the post and the CV as read.
pub struct Matching<'a> {
    pub db: &'a SqlitePool,
    pub taxonomy: &'a Taxonomy,
    pub user_id: i64,
    pub now: i64,
    pub laya: Option<&'a LayaClient>,
}

/// The match of job with profile, judged on llm where the taxonomy can't decide, and saved when
/// the post's and the CV's texts are given. new.cv_name "" is the CV's own name.
pub async fn ai_match(
    matching: &Matching<'_>,
    llm: Option<&Llm>,
    new: NewMatch<'_>,
    profile: &Profile,
    known: IndexMap<String, KnownTerm>,
) -> Result<AiMatch, LlmException> {
    let job = new.job;
    let matcher = Matcher::new(matching.taxonomy, profile.clone(), known);
    let groups = matcher::criteria(job);
    let mut verdicts = Verdicts::new();
    let mut undecided = Vec::new();
    for unit in matcher::units(&groups) {
        match matcher.decide(unit) {
            Some(verdict) => {
                verdicts.insert(unit.id.clone(), verdict);
            }
            None => undecided.push(Undecided {
                id: unit.id.clone(),
                text: unit.text.clone(),
                kind: unit.kind.clone(),
            }),
        }
    }
    match llm {
        Some(llm) if !undecided.is_empty() => {
            let cv_text = if profile.text_en.is_empty() {
                new.cv_text
            } else {
                &profile.text_en
            };
            let judged =
                match_prompts::judge(llm, &job.job_title, &undecided, cv_text, profile.location.trim()).await?;
            for verdict in judged {
                verdicts.insert(
                    verdict.id,
                    Verdict {
                        verdict: verdict.verdict,
                        by: "llm".to_string(),
                        method: "judgement".to_string(),
                        evidence: verdict.evidence,
                        reason: verdict.reason,
                    },
                );
            }
        }
        _ => {
            for unit in undecided {
                let verdict = if unit.kind == LOCATION {
                    unweighed_location()
                } else {
                    not_found()
                };
                verdicts.insert(unit.id, verdict);
            }
        }
    }
    let cv_name = if new.cv_name.is_empty() {
        profile.name.as_str()
    } else {
        new.cv_name
    };
    Ok(score_and_save(matching, NewMatch { cv_name, ..new }, &verdicts).await)
}

/// The verdicts scored, and saved as the user's match.
async fn score_and_save(matching: &Matching<'_>, new: NewMatch<'_>, verdicts: &Verdicts) -> AiMatch {
    let groups = matcher::criteria(new.job);
    let verdicts = matcher::combine(&groups, verdicts);
    let score = matcher::score(&groups, &verdicts);
    let laya_summary = matcher::laya_summary(new.job, &groups, &verdicts);
    // Laya's own reading of the verdicts, with its model; the score comes back without it if
    // Laya fails.
    let laya_reading = laya_client::reading(matching.laya, &laya_summary).await;
    let mut answer = AiMatch {
        laya_summary,
        soft_skills: matcher::soft_skills(new.job),
        apply_instructions: apply_instructions::read(new.job_text),
        laya_reading,
        saved_id: None,
        save_error: None,
        groups,
        verdicts,
        result: score,
    };
    if new.job_text.trim().is_empty() || new.cv_text.trim().is_empty() {
        return answer;
    }
    let saved = match_model::save(
        matching.db,
        matching.user_id,
        new,
        &answer.groups,
        &answer.verdicts,
        &answer.result,
        matching.now,
    )
    .await;
    match saved {
        Ok(id) => answer.saved_id = Some(id),
        Err(e) => {
            crate::log_line!("match (saving a match): {e}");
            answer.save_error = Some(format!("The match wasn't saved: {e}"));
        }
    }
    answer
}

fn not_found() -> Verdict {
    Verdict {
        verdict: "missing".to_string(),
        by: "taxonomy".to_string(),
        method: "not_found".to_string(),
        evidence: String::new(),
        reason: NOT_FOUND_REASON.to_string(),
    }
}

/// A location requirement nobody weighed (no AI key on for evaluating job posts): partial, so
/// it's shown without capping the score.
fn unweighed_location() -> Verdict {
    Verdict {
        verdict: "partial".to_string(),
        reason: UNWEIGHED_LOCATION_REASON.to_string(),
        ..not_found()
    }
}

/// What Laya's English checkpoint reads, shown under the AI match: both texts in English and the
/// verdicts, in the saved order, pretty-printed with four spaces.
pub fn laya_input(job: &Job, profile: &Profile, cv_text: &str, summary: &str) -> String {
    let cv = if profile.text_en.is_empty() {
        cv_text
    } else {
        &profile.text_en
    };
    let quoted = |text: &str| serde_json::Value::from(text).to_string();
    format!(
        "{{\n    \"job\": {},\n    \"cv\": {},\n    \"summary\": {}\n}}",
        quoted(&job.text_en),
        quoted(cv),
        quoted(summary)
    )
}
