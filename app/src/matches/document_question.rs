//! A question in a comment thread on a passage the candidate highlighted in a written
//! application or tailored résumé ("where does this come from?"), answered by the model that
//! writes that document from the same CV, post and notes, as a reviewer's question on a pull
//! request is. The answer comes with a suggested change, which joins the thread the next
//! revision is written on (document_writer::Revision). Nothing is saved.

use serde_json::json;
use sqlx::SqlitePool;

use super::document_box::RESUME;
use super::document_writer::{self, By, Comment, WriteError};
use super::match_model::SavedMatch;
use crate::llm::Effort;
use crate::matches::llm_key::KeyedLlm;

/// Longest passage that can be asked about, in characters: about a page of a letter.
pub const MAX_PASSAGE: usize = 4000;
/// Longest question, in characters.
pub const MAX_QUESTION: usize = 1000;
/// The longest answer may take, in tokens: a few paragraphs and a one-line comment.
const ANSWER_TOKENS: u32 = 2048;

/// What was asked about a passage.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// document_box::APPLICATION or RESUME.
    pub kind: &'static str,
    /// The whole document as it is in its box, edits included.
    pub document: String,
    /// The thread asked in: its passage, and what was said, the question last.
    pub comment: Comment,
    /// The notes in the panel now.
    pub notes: String,
}

/// The model's answer, and the change it suggests for the next revision ("" when nothing should
/// change).
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub answer: String,
    pub change: String,
}

impl Answer {
    /// The answer as its line in the thread, the suggested change after it.
    pub fn said(&self) -> String {
        if self.change.is_empty() {
            self.answer.clone()
        } else {
            format!("{}\nSuggested change: {}", self.answer, self.change)
        }
    }
}

impl Question {
    /// The question: the candidate's last line in the thread.
    pub fn asked(&self) -> &str {
        self.comment
            .said
            .iter()
            .rev()
            .find(|said| said.by == By::Candidate)
            .map_or("", |said| said.text.trim())
    }
}

/// What the document is called in the prompt.
fn document_name(kind: &str) -> &'static str {
    if kind == RESUME {
        "tailored CV (résumé)"
    } else {
        "job application (cover letter)"
    }
}

fn system(kind: &str) -> String {
    let document = document_name(kind);
    format!(
        r#"A candidate is checking the {document} you wrote for them from their CV and a job post, the way a reviewer comments on a pull request. They highlighted a passage of it and ask about it in a comment thread.

- Answer their last question about that passage, briefly and plainly, in the language of the question. The thread so far is given; don't repeat what was said in it.
- When they ask where it came from or whether it's true, point to what backs it: quote the line of the CV, the job post or the candidate's notes it rests on. When nothing does, or it goes beyond them (a number, a year count, a skill, a result), say so plainly: it was made up or inferred, and from what.
- When they ask for a change, say how you would change it.
- Then set change: one short instruction for the next revision of the {document} that fixes what the thread uncovered, e.g. "Say 9 years of professional experience (hired in 2017), never 16." Use facts the candidate states in the thread as true. Leave change empty when the passage is fine as it is.
- Never invent facts about the candidate."#
    )
}

/// Asks the question about the passage of saved match found's document.
pub async fn ask(
    db: &SqlitePool,
    found: &SavedMatch,
    llm: &KeyedLlm,
    question: &Question,
) -> Result<Answer, WriteError> {
    let cv = document_writer::written_from(db, found).await?;
    let user = format!(
        "Job post:

{job_text}

CV:

{cv}
{notes_block}

The {document}:

{text}

The highlighted passage:

{passage}

The comment thread on it so far:

{thread}

Answer the candidate's last question: {asked}",
        job_text = found.job_text,
        notes_block = document_writer::notes_block(&question.notes),
        document = document_name(question.kind),
        text = question.document.trim(),
        passage = question.comment.passage.trim(),
        thread = document_writer::thread_lines(&question.comment.said),
        asked = question.asked(),
    );
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["answer", "change"],
        "properties": {
            "answer": { "type": "string" },
            "change": { "type": "string" },
        },
    });
    let answer = llm
        .llm
        .structured(&system(question.kind), &user, &schema, Effort::Medium, ANSWER_TOKENS)
        .await?;
    let text = |field: &str| answer[field].as_str().unwrap_or("").trim().to_string();
    let answered = Answer {
        answer: text("answer"),
        change: text("change"),
    };
    if answered.answer.is_empty() {
        return Err(WriteError::Empty("The model returned an empty answer. Try again."));
    }
    Ok(answered)
}
