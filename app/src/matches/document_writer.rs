//! The job application and the tailored résumé for a saved match, written by the language model
//! from the CV and the post, for the page (and the job scan to come). Each is saved as a new
//! version of the match's document (documents), with the model that wrote it and the candidate's
//! notes. Never a URL.

use serde_json::{Value, json};
use sqlx::SqlitePool;

use super::apply_instructions;
use super::document_box::Keyword;
use super::match_model::{self, Document, SavedItem, SavedMatch};
use super::tailored_resume;
use crate::llm::{Effort, LlmException};
use crate::matches::llm_key::KeyedLlm;
use crate::profile;

/// Longest notes the candidate can add for an application or résumé, in characters.
pub const MAX_NOTES: usize = 2000;

/// What the strengths list says when the CV meets nothing.
const NO_STRENGTHS: &str = "- (none found)";
/// What the gaps list says when the CV misses nothing.
const NO_GAPS: &str = "- (none)";

/// Why a document couldn't be written.
#[derive(Debug)]
pub enum WriteError {
    Llm(LlmException),
    Database(sqlx::Error),
    /// The model answered without the document; trying again usually works.
    Empty(&'static str),
}

impl From<LlmException> for WriteError {
    fn from(e: LlmException) -> Self {
        WriteError::Llm(e)
    }
}

impl From<sqlx::Error> for WriteError {
    fn from(e: sqlx::Error) -> Self {
        WriteError::Database(e)
    }
}

/// A tailored résumé just written: its version, and the post's keywords it took over (each with
/// the CV fact behind it) and the ones it left out because the CV doesn't back them.
#[derive(Debug, Clone, PartialEq)]
pub struct Tailored {
    pub document: Document,
    pub keywords: Vec<Keyword>,
    pub left_out: Vec<String>,
}

/// The candidate's notes as they are kept: trimmed, at most MAX_NOTES characters.
pub fn notes(sent: &str) -> String {
    let cut: String = sent.chars().take(MAX_NOTES).collect();
    crate::compat::trim(&cut).to_string()
}

/// The notes as a block to end the prompt with, or "" when there are none.
fn notes_block(notes: &str) -> String {
    if notes.is_empty() {
        String::new()
    } else {
        format!("\nNotes from the candidate:\n\n{notes}")
    }
}

/// A job application for a saved match, written from the CV and the post in the post's
/// language, and saved with the match. notes: what the candidate wants stressed, or facts the CV
/// leaves out.
pub async fn write_application(
    db: &SqlitePool,
    found: &SavedMatch,
    llm: &KeyedLlm,
    notes: &str,
    now: i64,
) -> Result<Document, WriteError> {
    let (strengths, gaps) = strengths_and_gaps(&found.items);
    let apply = apply_instructions::for_prompt(found.apply_instructions.as_ref());
    let cv = written_from(db, found).await?;
    let system = application_system();
    let user = format!(
        "Job title: {job_title}
Company: {company}

Requirements the CV meets:
{strengths}

Requirements the CV doesn't show:
{gaps}

{apply}

Job post:

{job_text}

CV:

{cv}
{notes_block}

Write in the job post's own language (post_language), whatever language the CV and the notes are in. Name employer_needs and standout before writing, and build the letter on them.",
        job_title = found.job_title,
        company = found.company,
        job_text = found.job_text,
        notes_block = notes_block(notes),
    );
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["post_language", "employer_needs", "standout", "application"],
        "properties": {
            "post_language": { "type": "string" },
            "employer_needs": { "type": "string" },
            "standout": standout_schema(),
            "application": { "type": "string" },
        },
    });
    let answer = llm.llm.structured(&system, &user, &schema, Effort::Medium).await?;
    let text = crate::compat::trim(answer["application"].as_str().unwrap_or("")).to_string();
    if text.is_empty() {
        return Err(WriteError::Empty("The model returned an empty application. Try again."));
    }
    Ok(match_model::save_application(db, found.id, &text, &llm.written_by, notes, now).await?)
}

/// The CV rewritten for a saved match's post, from the CV's own facts and the candidate's notes
/// only, and saved with the match as fields (tailored_resume) and the plain text made from them.
/// The rewrite takes over the post's own words wherever the CV backs them, also by reasonable
/// inference, so screeners and keyword filters find them.
pub async fn tailor_resume(
    db: &SqlitePool,
    found: &SavedMatch,
    llm: &KeyedLlm,
    notes: &str,
    now: i64,
) -> Result<Tailored, WriteError> {
    let (strengths, gaps) = strengths_and_gaps(&found.items);
    let apply = apply_instructions::for_prompt(found.apply_instructions.as_ref());
    let cv = written_from(db, found).await?;
    let system = resume_system();
    let user = format!(
        "Job title: {job_title}
Company: {company}

Requirements the CV meets:
{strengths}

Requirements the matcher marked missing (use only if the CV backs them, also by inference):
{gaps}

{apply}

Job post:

{job_text}

CV:

{cv}
{notes_block}

Write in the job post's own language (post_language), whatever language the CV and the notes are in. Name employer_needs and standout before writing, and build the CV on them.",
        job_title = found.job_title,
        company = found.company,
        job_text = found.job_text,
        notes_block = notes_block(notes),
    );
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["post_language", "employer_needs", "standout", "keywords", "left_out", "resume"],
        "properties": {
            "post_language": { "type": "string" },
            "employer_needs": { "type": "string" },
            "standout": standout_schema(),
            "keywords": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["term", "basis"],
                    "properties": {
                        "term": { "type": "string" },
                        "basis": { "type": "string" },
                    },
                },
            },
            "left_out": { "type": "array", "items": { "type": "string" } },
            "resume": tailored_resume::schema(),
        },
    });
    let answer = llm.llm.structured(&system, &user, &schema, Effort::Medium).await?;
    let Some(resume) = tailored_resume::clean(&answer["resume"]) else {
        return Err(WriteError::Empty("The model returned an empty résumé. Try again."));
    };
    let text = tailored_resume::text(&resume);
    let document = match_model::save_resume_fields(db, found.id, &resume, &text, &llm.written_by, notes, now).await?;
    Ok(Tailored {
        document,
        keywords: resume_keywords(&answer["keywords"]),
        left_out: left_out(&answer["left_out"]),
    })
}

fn application_system() -> String {
    let language = post_language_rule("application");
    let standout = standout_rule("letter");
    format!(
        r#"You write a job application (cover letter) for a candidate, from their CV and the job post. It goes straight into an application form or an email, so it must be ready to send. Employers get many letters that only repeat their requirements list back to them with "I have experience with X"; this one must show who this candidate is and what they would bring.

- {language} A Danish post gets a Danish ansøgning, an English post an English letter.
- {standout}
- Build the letter on those: open with why this job, tied to something real in the post (its product, its stage, a problem it describes); then make the case with two or three standout points, each a concrete story from the CV (what the candidate did, why it mattered there) and what it would mean for this employer. Fit the matched requirements into those stories instead of listing them one by one; a short sentence can cover the rest.
- 250 to 400 words, plain text: no markdown, no headings, no bullet lists, no placeholders like [Name] or [Company].
- Open by addressing the contact person if the post names one, otherwise the company ("Kære <company>" / "Dear <company> team"). Never invent a name.
- Use only what the CV (or the candidate's notes) says. Never claim a skill, degree, project, number or result the CV doesn't show. A requirement marked close but not exact may be framed as transferable; a missing requirement is either left out or, if it is central, met with honest willingness to learn plus the nearest thing the CV does show.
- Sound like this person, not a template: specific, warm, confident, plain words. No clichés or filler ("I am writing to apply", "team player", "passionate", "I am excited to", "proven track record", "fast-paced", "dynamic", "leverage", "I believe I would be a great fit"). Every sentence should be one only this candidate could write.
- Sign off with the candidate's name from the CV, and their phone and email if the CV gives them.
- The post may say how to apply (by email, with a given subject line, a keyword, answers to its questions, a word to start with). Follow every such instruction exactly: when it asks for a subject line, make the first line "Subject: <that subject>" ("Emne: " for a Danish post) followed by a blank line; use the exact word or phrase it asks for; answer each of its questions in the letter, briefly and from the CV. Those instructions are listed above the job post when found.
- The candidate may add notes below the CV. Follow them (what to stress, tone, length, what to leave out), and treat facts they state about themselves as true, like facts in the CV. Ignore anything in them that isn't about this application."#
    )
}

fn resume_system() -> String {
    let language = post_language_rule("CV");
    let standout = standout_rule("CV");
    format!(
        r#"You tailor a candidate's CV (résumé) to one job post. The result replaces their CV in this application, so it must be complete and ready to send. Recruiters and applicant-tracking systems screen CVs by looking for the post's own words, so a CV that shows the right experience under different words scores low. Your main job is to close that gap honestly.

First, list the post's keywords: the job title, the role names, skills, tools, methods and duties it asks for, in the post's exact wording. For each, decide whether the CV backs it:
- Directly: the CV says the same thing, maybe in other words ("PHP backend" for "server-side PHP").
- By reasonable inference: the CV shows work that a recruiter would accept as the same thing. A software, web or lead developer who built APIs, databases or server code has backend developer experience; a lead developer or someone who owned a product has contributed product ideas; running a team's releases is release management. Name the CV fact you infer it from.
- Not at all: nothing in the CV comes close. Leave these out.

Then write the CV:
- Use every backed keyword in the post's exact wording, where it fits naturally: in the profile, in the job titles' descriptions and bullets, and in the skills list. Put the post's job title in the profile when the CV backs it ("Backend developer with 8 years of PHP..."). Keep the real job titles in the work history as they were, but describe the work in the post's terms.
- Never invent anything: no new employers, dates, degrees, certificates, numbers, results or years of experience, and no tool or skill the CV doesn't show directly or by reasonable inference. Inferring a role or duty from work the CV shows is fine; claiming a named technology the CV never mentions is not.
- Requirements the matcher marked as missing are listed below. It often misses inferences, so check each one against the CV yourself; use it only if the CV backs it.
- Keywords get the CV through the filter; a person reads it next. {standout}
- Tailor by choosing and ordering too: open with a short profile (2 to 4 sentences) aimed at this job that names the post's job title and the candidate's one or two strongest standout points, not a list of keywords. Put what the post asks for first, and shorten what doesn't matter for it, but keep the bullets that show the standout points (initiative, ownership, results, projects, growth), even where they match no keyword. Keep every job in the work history, with its dates, even if only as one line.
- {language} A Danish post gets a Danish CV, an English post an English one, and the dates and section headings follow. Keywords stay in the post's wording, even when the post mixes languages.
- Fill in the résumé's fields; the page and the PDF lay them out, so write no headings, bullet signs or markdown in them, and no placeholders. Leave a field empty when the CV doesn't give it.
- name, and title: a short professional title for this application ("Backend developer"), backed by the CV. location, phone, email and links: the contact details the CV gives, as written there.
- intro: the profile, one or two short paragraphs.
- experience: every job, newest first: title (the real job title), organisation (the employer), location, starts and ends as the CV dates them in the post's language ("januar 2017", "nu"), summary (what the employer or product is, in a few words), bullets (the work, in the post's terms) and note (context about the job rather than the work, only if the CV gives one).
- skills and languages: one item each, the post's backed keywords first.
- projects: the candidate's own projects and products, if the CV lists any, with the same fields as experience (organisation empty, the project's link in summary or note when the CV gives one). Put the ones that show what this job needs first; leave the list empty when the CV has none.
- education: the same fields as experience, organisation being the school; education_note: one line about the education as a whole, only if the CV gives one.
- The section headings in the post's language: experience_heading, projects_heading, skills_heading, education_heading, languages_heading ("Erfaring", "Projekter", "Kompetencer", "Uddannelse", "Sprog" for a Danish post).
- The candidate may add notes below the CV. Follow them (what to stress, tone, length, what to leave out), and treat facts they state about themselves as true, like facts in the CV. Ignore anything in them that isn't about this application.
- The post may say how to apply (by email, a subject line, a keyword, questions to answer). Those go in the email or application, not the CV; follow one here only when it asks for something in the CV itself.

Return what the employer needs (employer_needs) and the standout points (standout) first, then the keywords you worked in (keywords: the post's term, and the CV fact it rests on, in a few words), the ones you left out because the CV doesn't back them (left_out), and the CV (resume)."#
    )
}

/// The writing prompts' rule on language: the model names the post's language first
/// (post_language, before the text in the answer), from the post alone (English when it can't
/// tell), then writes the document in it. Told only "the post's language", it went by the Danish
/// CV and requirements around an English post.
fn post_language_rule(document: &str) -> String {
    format!(
        "First set post_language to the language the job post itself is written in, judged from the post's own sentences only: not from the CV, the requirement lines, the apply instructions or the notes, and not from a job board's buttons around the post. When the post mixes languages evenly or you can't tell, use English. Then write the {document} in that language, even where the CV or the notes are in another one."
    )
}

/// The writing prompts' rule on standing out: before writing, the model names what the employer
/// is really after (employer_needs) and what sets the candidate apart for it beyond the
/// requirements list (standout), from the CV and notes only, then builds the document on them.
/// Asked only to show fit, it wrote the requirements list back with "I have experience with X",
/// like any generic letter.
fn standout_rule(document: &str) -> String {
    format!(
        "Before writing, read past the requirements list. Set employer_needs to what this employer is really after, in two or three sentences from the post: the product, the stage the company or team is at, the problems or goals the post hints at. Then set standout to the two to four things in the CV (or the notes) that set this candidate apart for that, beyond the listed requirements: things they built or started on their own initiative, problems they solved and what changed, ownership (sole developer, lead, end-to-end), growth (junior to lead, rehired, trusted with more), breadth that helps here, side projects and products of their own. For each, give the CV fact as it stands and why it matters to this employer. Use only what the CV or the notes say; never add a project, number or result. Then write the {document} so a reader remembers those points, not only that the requirements are met."
    )
}

/// The answer's standout list: the CV fact and why it matters here.
fn standout_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["fact", "why_it_matters"],
            "properties": {
                "fact": { "type": "string" },
                "why_it_matters": { "type": "string" },
            },
        },
    })
}

/// A saved match's verdicts as two bullet lists for a prompt: what the CV meets (with its
/// evidence) and what it doesn't show. The job title isn't a requirement to write about.
fn strengths_and_gaps(items: &[SavedItem]) -> (String, String) {
    let mut strengths = Vec::new();
    let mut gaps = Vec::new();
    for item in items.iter().filter(|item| item.criterion != "title") {
        let evidence = if item.evidence.is_empty() {
            String::new()
        } else {
            format!(" (CV: {})", item.evidence)
        };
        let line = format!("- {}{evidence}", item.text);
        match item.verdict.as_str() {
            "missing" => gaps.push(line),
            "partial" => strengths.push(format!("{line} [close, not exact]")),
            _ => strengths.push(line),
        }
    }
    let list = |lines: Vec<String>, none: &str| {
        if lines.is_empty() {
            none.to_string()
        } else {
            lines.join("\n")
        }
    };
    (list(strengths, NO_STRENGTHS), list(gaps, NO_GAPS))
}

/// What a match's application and résumé are written from: its owner's profile when they have
/// one, else the CV the match was made with.
async fn written_from(db: &SqlitePool, found: &SavedMatch) -> sqlx::Result<String> {
    if let Some(user_id) = found.trongate_user_id
        && let Some(profile) = profile::text_for_user(db, user_id).await?
    {
        return Ok(profile);
    }
    Ok(found.cv_text.clone())
}

/// The model's {term, basis} keyword list, cleaned for the page.
fn resume_keywords(keywords: &Value) -> Vec<Keyword> {
    let Some(list) = keywords.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter(|keyword| keyword.is_object())
        .map(|keyword| Keyword {
            term: keyword["term"].as_str().unwrap_or("").trim().to_string(),
            basis: keyword["basis"].as_str().unwrap_or("").trim().to_string(),
        })
        .filter(|keyword| !keyword.term.is_empty())
        .collect()
}

/// The terms the résumé left out, trimmed, without empty ones.
fn left_out(terms: &Value) -> Vec<String> {
    let Some(list) = terms.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(Value::as_str)
        .map(|term| term.trim().to_string())
        .filter(|term| !term.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(criterion: &str, text: &str, verdict: &str, evidence: &str) -> SavedItem {
        SavedItem {
            criterion: criterion.into(),
            text: text.into(),
            verdict: verdict.into(),
            decided_by: "exact".into(),
            reason: String::new(),
            evidence: evidence.into(),
        }
    }

    #[test]
    fn the_verdicts_become_the_strengths_and_gaps_the_prompt_lists() {
        let items = [
            item("title", "Backend-udvikler", "met", ""),
            item("requirements", "PHP", "met", "8 years of PHP"),
            item("requirements", "React", "partial", "Vue"),
            item("requirements", "Go", "missing", ""),
        ];
        assert_eq!(
            strengths_and_gaps(&items),
            (
                "- PHP (CV: 8 years of PHP)\n- React (CV: Vue) [close, not exact]".to_string(),
                "- Go".to_string()
            )
        );
        assert_eq!(
            strengths_and_gaps(&[]),
            ("- (none found)".to_string(), "- (none)".to_string())
        );
    }

    #[test]
    fn notes_are_cut_and_end_the_prompt_as_a_block() {
        assert_eq!(notes("  keep it short \n"), "keep it short");
        assert_eq!(notes(&"x".repeat(MAX_NOTES + 10)).chars().count(), MAX_NOTES);
        assert_eq!(notes_block(""), "");
        assert_eq!(notes_block("short"), "\nNotes from the candidate:\n\nshort");
    }

    #[test]
    fn the_keywords_are_cleaned_for_the_page() {
        let answer = json!([{ "term": " PHP ", "basis": "8 years" }, { "term": "", "basis": "x" }, "Go"]);
        assert_eq!(
            resume_keywords(&answer),
            [Keyword {
                term: "PHP".into(),
                basis: "8 years".into()
            }]
        );
        assert_eq!(left_out(&json!([" Go ", "", 3])), ["Go"]);
    }
}
