//! The three language-model reads the matching runs on: a job post into its requirements list, a CV
//! into a profile, and a verdict on each requirement the taxonomy couldn't decide. Each takes the
//! Llm to call, so the caller decides which key it runs on.

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Job, Profile, seniority};
use crate::llm::{Effort, Llm, LlmException};

const READ_JOB: &str = r#"You read job posts (often Danish) for a candidate and list what the employer asks for. Use only what the post says.

requirements is one list of what a candidate must or should have. Keep each entry atomic: one skill, tool, language, certificate, education, amount of experience or trait ("Erfaring med PHP og Laravel" is two entries).
- value names it as the post does, without qualifiers like "erfaring med" or "kendskab til".
- kind: skill, education, certificate, experience, language, soft_skill for personal traits ("selvstændig", "god til at samarbejde"), or location for where the candidate must live or show up ("You need to be located close to Copenhagen", "bosat i Nordjylland", "3 dage om ugen på kontoret i Aarhus"). value names the place and how it is asked ("Located close to Copenhagen", "On site in Aarhus 3 days a week"). Only the employer's office address, or a post that allows full remote work, is no location requirement.
- required is false for nice-to-haves ("en fordel", "gerne", "a plus").
- Alternatives share an alt_group label ("pædagog eller pædagogisk assistent": two entries, alt_group "a"; "uddannet kok eller 5 års erfaring": an education and an experience entry, alt_group "b"); alt_group is empty otherwise.
- min_years: the years of experience asked for ("et par år" = 2, "flere års" = 3, none stated = 0).
- min_level: the level asked for, in English, when the post states one ("flydende" = "fluent", "modersmål" = "native", "kandidat" = "master's degree", "senior"); empty otherwise.

working_language: the language the job is done in, named in English ("Danish", "English", "Finnish"). It is the language the post's own sentences are written in, even when the post never asks for it: a post in Finnish expects a Finnish speaker. Only when the post names another working language ("our working language is English", "you don't need to speak Danish") is it that one. Judge it from the post's own text, not from a job board's buttons around it.

level: the career level the post is for, when it says so: "student" (studiejob, studentermedhjælper, elev, lærling), "intern" (praktik, internship), "graduate" (nyuddannet, trainee), "junior", "senior" or "lead" (lead, principal, teamleder); empty when it states none or several.

responsibilities: at most 6, a few words each.
job_title_en and english: the job title and each value in English (the same text when it already is English)."#;

const READ_CV: &str = r#"You read a CV and list what it shows the candidate can do. Use only what the CV says or directly shows: a bullet like "Upgraded Symfony from 6 to 7" shows Symfony and PHP.

Keep each skill atomic (one technology, tool, method or language per item) and name it as commonly written ("Laravel", "Kubernetes", "CI/CD").
Name each list by the kind of term it holds: title (job titles held), skill, language, certificate (certifications), education.
experience_years: total years of professional work, from the dates.
location: where the candidate lives, as the CV gives it (town and postcode as written); empty when it doesn't say.
Include the languages the CV is written in or names.
text_en: the whole CV as plain English text, translated if it is in another language, otherwise as given; keep every fact, drop layout."#;

const JUDGE: &str = r#"You judge whether a candidate meets job requirements, from their CV alone. For each requirement say:
- "met" when the CV shows it (directly, or through something that clearly includes it: Laravel shows PHP),
- "partial" when the CV shows something close or transferable but not the thing itself (Vue for React, Symfony for Laravel),
- "missing" when the CV shows nothing relevant.

A location requirement is about where the candidate lives, against the place and commute the post asks for:
- "met" when they live there or within an easy daily commute (about 45 minutes),
- "partial" when the commute is longer but the post doesn't ask for someone close or local, or when nothing says where they live,
- "missing" when the post asks for someone close, local or living in the area and they live farther (Slagelse is about 100 km from Copenhagen: "missing" for "located close to Copenhagen").
Its reason names both places and the rough distance or commute.

A language requirement is about the languages the candidate speaks, judged from "The candidate's languages" line only: it lists the languages the original CV is written in or names. The CV below may be an English translation, so the language it is written in says nothing.
- "met" when that line names the language (at the level asked, if any; the CV may say the level),
- "partial" when the CV gives it at a lower level than asked, or the line names a closely related language the candidate could work in (Norwegian or Swedish for Danish),
- "missing" otherwise: a language is no skill to infer, and Danish is no Finnish.

Be strict: a guess is "missing". evidence quotes the CV briefly, empty when missing. reason is one short sentence in English."#;

/// The longest answer each read may write, in tokens: about twice the longest a local model wrote
/// for it in the model bake-off, so one that repeats a list until its context is full stops in a
/// few minutes instead of at the call's time limit.
const READ_JOB_TOKENS: u32 = 4096;
/// A CV's read holds its whole text again in English (text_en), so a long CV takes more.
const READ_CV_TOKENS: u32 = 8192;
const JUDGE_TOKENS: u32 = 4096;

/// What judge() says where the CV doesn't say where the candidate lives.
const LIVES_NOT_STATED: &str = "not stated";

/// What judge() says where the CV's read found no languages.
const LANGUAGES_NOT_STATED: &str = "none stated";

/// A requirement the taxonomy couldn't decide, for the model to judge.
#[derive(Debug, Clone, PartialEq)]
pub struct Undecided {
    pub id: String,
    pub text: String,
    pub kind: String,
}

/// The model's verdict on one requirement.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Judged {
    pub id: String,
    pub verdict: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Deserialize)]
struct Judgement {
    #[serde(default)]
    verdicts: Vec<Judged>,
}

/// A job post as { job_title, job_title_en, company, requirements, responsibilities, level,
/// working_language }. The post isn't translated: written first, a translation turned the
/// requirements' own words English.
pub async fn read_job(llm: &Llm, text: &str) -> Result<Job, LlmException> {
    let answer = llm
        .structured(
            READ_JOB,
            &format!("Job post:\n\n{text}"),
            &job_schema(),
            Effort::Low,
            READ_JOB_TOKENS,
        )
        .await?;
    shaped(answer, "job post")
}

/// A CV as a profile: { name, location, title, skill, language, certificate, education,
/// experience_years, responsibilities, text_en }.
pub async fn read_cv(llm: &Llm, text: &str) -> Result<Profile, LlmException> {
    let answer = llm
        .structured(
            READ_CV,
            &format!("CV:\n\n{text}"),
            &cv_schema(),
            Effort::Low,
            READ_CV_TOKENS,
        )
        .await?;
    shaped(answer, "CV")
}

/// The model's verdict on each requirement the taxonomy couldn't decide. location is where the
/// candidate lives (as the CV gives it), for location requirements; "" when it isn't known.
/// languages are the ones the original CV is written in or names (Profile::language), for
/// language requirements: cv_text is often the CV's English translation, which would make
/// English look spoken and hide the CV's own language.
pub async fn judge(
    llm: &Llm,
    job_title: &str,
    items: &[Undecided],
    cv_text: &str,
    location: &str,
    languages: &[String],
) -> Result<Vec<Judged>, LlmException> {
    let list = items
        .iter()
        .map(|item| format!("- [{}] ({}) {}", item.id, item.kind, item.text))
        .collect::<Vec<_>>()
        .join("\n");
    let lives = if location.is_empty() {
        LIVES_NOT_STATED
    } else {
        location
    };
    let spoken = spoken_languages(languages);
    let user = format!(
        "Job title: {job_title}\nThe candidate lives in: {lives}\nThe candidate's languages: {spoken}\n\nRequirements:\n{list}\n\nCV:\n\n{cv_text}"
    );
    let answer = llm
        .structured(JUDGE, &user, &judge_schema(), Effort::Medium, JUDGE_TOKENS)
        .await?;
    Ok(shaped::<Judgement>(answer, "verdicts")?.verdicts)
}

/// The candidate's languages as judge() names them: "Dansk, Engelsk", or LANGUAGES_NOT_STATED.
fn spoken_languages(languages: &[String]) -> String {
    let named: Vec<&str> = languages.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if named.is_empty() {
        LANGUAGES_NOT_STATED.to_string()
    } else {
        named.join(", ")
    }
}

/// The model's answer as T, or an error saying it wasn't the shape asked for.
fn shaped<T: for<'de> Deserialize<'de>>(answer: Value, what: &str) -> Result<T, LlmException> {
    serde_json::from_value(answer).map_err(|e| {
        crate::log_line!("match: the model's {what} didn't have the shape asked for: {e}");
        LlmException::retryable(format!(
            "The model's reading of the {what} didn't have the shape asked for ({e}). Try again."
        ))
    })
}

/// A list of strings, as the schemas write it.
fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

/// One requirements list, as agreed for job_post_terms: kind, value, required, alt_group
/// (alternatives share a label), min_years and min_level. Soft skills are entries of kind
/// soft_skill.
fn job_schema() -> Value {
    let requirement = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["value", "english", "kind", "required", "alt_group", "min_years", "min_level"],
        "properties": {
            "value": { "type": "string" },
            "english": { "type": "string" },
            "kind": { "type": "string", "enum": ["skill", "education", "certificate", "experience", "language", "soft_skill", "location"] },
            "required": { "type": "boolean" },
            "alt_group": { "type": "string" },
            "min_years": { "type": "integer" },
            "min_level": { "type": "string" },
        },
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["job_title", "job_title_en", "company", "requirements", "responsibilities", "level", "working_language"],
        "properties": {
            "job_title": { "type": "string" },
            "job_title_en": { "type": "string" },
            "company": { "type": "string" },
            "requirements": { "type": "array", "items": requirement },
            "responsibilities": strings(),
            "level": { "type": "string", "enum": level_choices() },
            "working_language": { "type": "string" },
        },
    })
}

/// What extract_job's level may be: one of seniority::LEVELS, or "" for none.
fn level_choices() -> Vec<&'static str> {
    std::iter::once("").chain(seniority::LEVELS).collect()
}

fn cv_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["name", "location", "title", "skill", "language", "certificate", "education", "experience_years", "responsibilities", "text_en"],
        "properties": {
            "name": { "type": "string" },
            "location": { "type": "string" },
            "text_en": { "type": "string" },
            "title": strings(),
            "skill": strings(),
            "language": strings(),
            "certificate": strings(),
            "education": strings(),
            "experience_years": { "type": "integer" },
            "responsibilities": strings(),
        },
    })
}

fn judge_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["verdicts"],
        "properties": {
            "verdicts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "verdict", "evidence", "reason"],
                    "properties": {
                        "id": { "type": "string" },
                        "verdict": { "type": "string", "enum": ["met", "partial", "missing"] },
                        "evidence": { "type": "string" },
                        "reason": { "type": "string" },
                    },
                },
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_judge_gets_the_cvs_own_languages() {
        let languages = vec!["Dansk".to_string(), " ".to_string(), "Engelsk".to_string()];
        assert_eq!(spoken_languages(&languages), "Dansk, Engelsk");
        assert_eq!(spoken_languages(&[]), LANGUAGES_NOT_STATED);
    }
}
