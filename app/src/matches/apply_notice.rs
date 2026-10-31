//! What the post asks of applicants beyond a click (apply_instructions), as the result prints it
//! above the requirements, ported from modules/cv_match/Apply_notice.php: send it by email, with
//! a subject line, a keyword or answers to questions, quoting the post. Often a filter for
//! people who only press Easy Apply.

use super::application_delivery;
use super::apply_instructions::{self, ApplyInstructions};

pub const HEADING: &str = "This post asks for applications by email";
pub const TRAP_HEADING: &str = "Read this before you apply: the post asks for more than a click";
pub const TRAP_NOTE: &str = "Posts like this often skip applications that don't follow it, such as Easy Apply.";
pub const WRITER_NOTE: &str = "Write application below follows it for you.";

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyNotice {
    pub trap: bool,
    pub heading: &'static str,
    pub note: String,
    pub email: String,
    pub mailto: String,
    pub subject: String,
    /// The word to include, unless the subject line already carries it.
    pub keyword: String,
    /// "It asks for a specific subject line, answers to questions." or ''.
    pub asks: String,
    pub lines: Vec<String>,
}

impl ApplyNotice {
    /// None when the post asks for nothing special. writer: the page can write the application.
    pub fn from(apply: Option<&ApplyInstructions>, writer: bool) -> Option<Self> {
        let apply = apply?;
        let note = match (apply.trap, writer) {
            (false, _) => String::new(),
            (true, true) => format!("{TRAP_NOTE} {WRITER_NOTE}"),
            (true, false) => TRAP_NOTE.to_string(),
        };
        let words: Vec<&str> = apply
            .asks
            .iter()
            .map(|ask| apply_instructions::ask_words(ask))
            .collect();
        Some(ApplyNotice {
            trap: apply.trap,
            heading: if apply.trap { TRAP_HEADING } else { HEADING },
            note,
            email: apply.email.clone(),
            mailto: if apply.email.is_empty() {
                String::new()
            } else {
                application_delivery::mailto(&apply.email, &apply.subject)
            },
            subject: apply.subject.clone(),
            keyword: if apply.subject.is_empty() {
                apply.keyword.clone()
            } else {
                String::new()
            },
            asks: if words.is_empty() {
                String::new()
            } else {
                format!("It asks for {}.", words.join(", "))
            },
            lines: apply.lines.clone(),
        })
    }
}
