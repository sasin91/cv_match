//! The box for one document of a saved match, the job application or the tailored résumé, as
//! the result prints it, ported from modules/cv_match/Document_box.php: its title, the Write
//! button, the text already written (editable, with Copy and Download PDF), where the post
//! wants the application sent, and a note when the key is switched off for writing it. Just
//! after the résumé is written, with_keywords() adds the post's keywords it took over and the
//! ones it left out (tailor_resume).
//!
//! Unlike the PHP app, writing again keeps the text before: the box lists the earlier versions
//! (documents), newest first, each with when and by which model it was written, the notes it was
//! written with, and its text to copy or download as a PDF.

use super::application_delivery;
use super::apply_instructions::ApplyInstructions;
use super::dates;
use super::match_model::Version;

/// The kind of document that is a job application (also documents.kind).
pub const APPLICATION: &str = "application";
/// The kind of document that is a tailored résumé (also documents.kind).
pub const RESUME: &str = "resume";

/// The documents in the order the panel shows them: kind, title, noun, Write button.
pub const KINDS: [(&str, &str, &str, &str); 2] = [
    (APPLICATION, "Job application", "application", "Write application"),
    (RESUME, "Tailored résumé", "résumé", "Tailor résumé"),
];
pub const WRITE_AGAIN: &str = "Write it again";
pub const EARLIER_VERSIONS: &str = "Earlier versions";

/// A keyword the résumé took over from the post, and the CV fact it rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct Keyword {
    pub term: String,
    pub basis: String,
}

/// The keywords the résumé took over: "Uses 2 of the post's keywords: PHP, Go", and each one.
#[derive(Debug, Clone, PartialEq)]
pub struct Keywords {
    pub summary: String,
    pub items: Vec<Keyword>,
}

/// One earlier version of the document, as its box lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct EarlierVersion {
    /// Its row in documents, which its PDF is made from.
    pub id: i64,
    /// ISO 8601, which the page shows in local time.
    pub written_at: String,
    /// "openai/gpt-5.4-mini"; "" when it came from the PHP app.
    pub written_by: String,
    pub notes: String,
    pub text: String,
}

impl From<&Version> for EarlierVersion {
    fn from(version: &Version) -> Self {
        EarlierVersion {
            id: version.id,
            written_at: dates::iso(version.written_at),
            written_by: version.written_by.clone(),
            notes: version.notes.clone(),
            text: version.text.clone(),
        }
    }
}

/// What a saved match has written of one document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Written {
    pub text: String,
    /// Unix seconds as text, '' when never written.
    pub written_at: String,
    /// The résumé was written as fields (match_resumes), so its PDF is laid out from them.
    pub structured: bool,
    /// The versions before this one, newest first.
    pub earlier: Vec<Version>,
}

/// Where the post wants the application sent.
#[derive(Debug, Clone, PartialEq)]
pub struct SendTo {
    pub email: String,
    pub mailto: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentBox {
    pub kind: &'static str,
    pub id: i64,
    pub title: &'static str,
    pub write: &'static str,
    pub off: bool,
    pub off_note: String,
    pub text: String,
    pub written_at: String,
    pub structured: bool,
    pub send_to: Option<SendTo>,
    pub keywords: Option<Keywords>,
    pub left_out: String,
    pub earlier_heading: &'static str,
    pub earlier: Vec<EarlierVersion>,
}

impl DocumentBox {
    /// on: whether the key may write it.
    pub fn from(kind: &str, id: i64, written: &Written, on: bool, apply: Option<&ApplyInstructions>) -> Self {
        let (kind, title, noun, write) = *KINDS.iter().find(|(k, ..)| *k == kind).expect("a kind of KINDS");
        DocumentBox {
            kind,
            id,
            title,
            write: if written.text.is_empty() { write } else { WRITE_AGAIN },
            off: !on,
            off_note: if on {
                String::new()
            } else {
                format!("Your API key isn't on for writing the {noun}.")
            },
            text: written.text.clone(),
            written_at: written.written_at.clone(),
            structured: written.structured,
            send_to: if kind == APPLICATION { send_to(apply) } else { None },
            keywords: None,
            left_out: String::new(),
            earlier_heading: EARLIER_VERSIONS,
            earlier: written.earlier.iter().map(EarlierVersion::from).collect(),
        }
    }

    /// The box with the keywords the résumé took over and the ones it left out.
    pub fn with_keywords(mut self, keywords: Vec<Keyword>, left_out: &[String]) -> Self {
        self.keywords = (!keywords.is_empty()).then(|| Keywords {
            summary: format!(
                "Uses {} of the post's keywords: {}",
                keywords.len(),
                keywords.iter().map(|k| k.term.as_str()).collect::<Vec<_>>().join(", ")
            ),
            items: keywords,
        });
        self.left_out = if left_out.is_empty() {
            String::new()
        } else {
            format!("Left out, since your CV doesn't show it: {}", left_out.join(", "))
        };
        self
    }
}

fn send_to(apply: Option<&ApplyInstructions>) -> Option<SendTo> {
    let apply = apply.filter(|apply| !apply.email.is_empty())?;
    Some(SendTo {
        email: apply.email.clone(),
        mailto: application_delivery::mailto(&apply.email, &apply.subject),
        subject: apply.subject.clone(),
    })
}
