//! The box for one document of a saved match, the job application or the tailored résumé, as
//! the result prints it, ported from modules/cv_match/Document_box.php: its title, the Write
//! button, the text already written (editable, with Copy and Download PDF), where the post
//! wants the application sent, and a note when the key is switched off for writing it.

use super::application_delivery;
use super::apply_instructions::ApplyInstructions;

pub const APPLICATION: &str = "application";
pub const RESUME: &str = "resume";

/// The documents in the order the panel shows them: kind, title, noun, Write button.
pub const KINDS: [(&str, &str, &str, &str); 2] = [
    (APPLICATION, "Job application", "application", "Write application"),
    (RESUME, "Tailored résumé", "résumé", "Tailor résumé"),
];
pub const WRITE_AGAIN: &str = "Write it again";

/// What a saved match has written of one document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Written {
    pub text: String,
    /// Unix seconds as text, '' when never written.
    pub written_at: String,
    /// The résumé was written as fields (cv_match_resumes), so its PDF is laid out from them.
    pub structured: bool,
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
                format!("Writing the {noun} needs your own API key, which comes to the desktop app with the AI match.")
            },
            text: written.text.clone(),
            written_at: written.written_at.clone(),
            structured: written.structured,
            send_to: if kind == APPLICATION { send_to(apply) } else { None },
        }
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
