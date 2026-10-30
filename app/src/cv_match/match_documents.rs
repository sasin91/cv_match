//! The "Application and résumé" panel of a saved match, ported from
//! modules/cv_match/Match_documents.php: the notes for the AI, a box per document (DocumentBox),
//! the Apply button and the Jobnet joblog. A match that wasn't saved gets the reason instead,
//! since nothing can be written for it.

use super::apply_instructions::ApplyInstructions;
use super::cv_match_model::SavedMatch;
use super::document_box::{self, DocumentBox, Written};

pub const NOT_SAVED: &str = "This match wasn't saved, so nothing can be written for it.";
pub const INTRO: &str = "Saved. Write a job application or a résumé tailored to this post, from your CV. Tailoring the résumé writes the application too, when there is none yet:";
pub const NOTES_LABEL: &str = "Notes for the application and résumé";
pub const NOTES_HINT: &str = "Optional: anything the AI should know or stress. E.g. \"I can start right away\", \"stress my Laravel work\", \"keep it short\".";
pub const NOTES_MAX: usize = 2000;

/// Which documents the key may write.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Uses {
    pub resume: bool,
    pub application: bool,
    pub job_evaluation: bool,
}

impl Uses {
    pub fn of(&self, kind: &str) -> bool {
        match kind {
            document_box::RESUME => self.resume,
            document_box::APPLICATION => self.application,
            _ => self.job_evaluation,
        }
    }

    pub fn any(&self) -> bool {
        self.resume || self.application || self.job_evaluation
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchDocuments {
    pub id: Option<i64>,
    pub error: String,
    pub boxes: Vec<DocumentBox>,
    pub intro: &'static str,
    pub notes_label: &'static str,
    pub notes_hint: &'static str,
    pub notes_max: usize,
}

impl MatchDocuments {
    /// written: (application, résumé) as the match has them.
    pub fn from(
        id: Option<i64>,
        save_error: &str,
        written: &(Written, Written),
        uses: Uses,
        apply: Option<&ApplyInstructions>,
    ) -> Self {
        let (error, boxes) = match id {
            None => (
                if save_error.is_empty() {
                    NOT_SAVED.to_string()
                } else {
                    save_error.to_string()
                },
                Vec::new(),
            ),
            Some(id) => (
                String::new(),
                document_box::KINDS
                    .iter()
                    .map(|(kind, ..)| {
                        let written = if *kind == document_box::APPLICATION {
                            &written.0
                        } else {
                            &written.1
                        };
                        DocumentBox::from(kind, id, written, uses.of(kind), apply)
                    })
                    .collect(),
            ),
        };
        MatchDocuments {
            id,
            error,
            boxes,
            intro: INTRO,
            notes_label: NOTES_LABEL,
            notes_hint: NOTES_HINT,
            notes_max: NOTES_MAX,
        }
    }
}

/// What a saved match has written, as from() takes it.
pub fn written_of(found: &SavedMatch) -> (Written, Written) {
    let at = |at: Option<i64>| at.map(|at| at.to_string()).unwrap_or_default();
    (
        Written {
            text: found.application_text.clone().unwrap_or_default(),
            written_at: at(found.application_written_at),
            structured: false,
        },
        Written {
            text: found.resume_text.clone().unwrap_or_default(),
            written_at: at(found.resume_written_at),
            structured: found.resume_structured,
        },
    )
}
