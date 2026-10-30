//! A saved match's Jobnet joblog entry (joblog::read) as the panel under its application and
//! résumé prints it (templates/cv_match/joblog_panel.html), ported from
//! modules/cv_match/Joblog_panel.php: Jobnet's sections with each field to copy, a choice with
//! its options and the one to pick, the three PDFs, the "Fill it in with an AI" prompt and
//! whether the candidate has entered it in their joblog (joblog_logged.html).
//!
//! Each option carries what the AI prompt says for it; js/app.js reads typed values from the
//! inputs, edits included.

use super::dates;
use super::joblog::{self, Entry, Field, File, Value};

// The candidate picks how far they are here, saved with the match (joblog_status).
pub const CHOOSE: &str = "searchStatus";
pub const CHOOSE_HINT: &str = "Pick it here as you get along: it's saved with the match.";
pub const CHECKBOX_YES: &str = "Ja";
pub const CHECKBOX_NO: &str = "Nej";
pub const CHARACTERS_LEFT: &str = "Antal tegn tilbage:";
pub const LOGGED_LABEL: &str = "I've entered this job in my joblog on Jobnet";
pub const LOGGED_ON: &str = "Entered on";

#[derive(Debug, Clone, PartialEq)]
pub struct JoblogPanel {
    pub id: i64,
    pub sections: Vec<PanelSection>,
    pub files: Vec<PanelFile>,
    /// "Jobansøgning: Skriv ansøgningen først (Write application)." for each PDF that can't be
    /// made yet.
    pub missing: Vec<String>,
    pub logged: Logged,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelSection {
    pub heading: &'static str,
    pub fields: Vec<PanelField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelField {
    pub name: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub hint: String,
    /// Picked from options rather than typed.
    pub choice: bool,
    /// The choice the candidate makes here (CHOOSE).
    pub choose: bool,
    pub options: Vec<PanelOption>,
    pub value: String,
    pub multiline: bool,
    pub numeric: bool,
    /// Characters left of Jobnet's limit, for the notes.
    pub left: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelOption {
    pub value: &'static str,
    pub text: &'static str,
    pub chosen: bool,
    pub prompt: String,
}

/// A PDF button: its title says what it is, or why it can't be made yet.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelFile {
    pub kind: &'static str,
    pub label: String,
    pub ready: bool,
    pub title: &'static str,
}

/// Whether, and when (ISO time or ''), it is in the candidate's joblog, and the day applied as
/// the date field says it (joblog::applied_on), which the page copies there when the status is
/// saved.
#[derive(Debug, Clone, PartialEq)]
pub struct Logged {
    pub checked: bool,
    pub at: String,
    pub applied_on: String,
}

impl JoblogPanel {
    pub fn from(id: i64, entry: &Entry) -> Self {
        JoblogPanel {
            id,
            sections: entry
                .sections
                .iter()
                .map(|section| PanelSection {
                    heading: section.heading,
                    fields: section.fields.iter().map(field).collect(),
                })
                .collect(),
            files: entry.files.iter().map(file).collect(),
            missing: entry
                .files
                .iter()
                .filter(|file| !file.ready)
                .map(|file| format!("{}: {}", file.label, file.missing))
                .collect(),
            logged: Logged::from(entry.logged_at, applied_on(entry)),
        }
    }
}

impl Logged {
    pub fn from(logged_at: Option<i64>, applied_on: String) -> Self {
        Logged {
            checked: logged_at.is_some(),
            at: logged_at.map(dates::iso).unwrap_or_default(),
            applied_on,
        }
    }
}

/// The entry's submissionDate value.
fn applied_on(entry: &Entry) -> String {
    entry
        .sections
        .iter()
        .flat_map(|section| &section.fields)
        .find(|field| field.name == "submissionDate")
        .map(|field| field.value.text().to_string())
        .unwrap_or_default()
}

fn field(field: &Field) -> PanelField {
    let choice = matches!(field.kind, "radio" | "checkbox");
    let choose = field.name == CHOOSE;
    let multiline = field.kind == "textarea";
    let value = if choice {
        String::new()
    } else {
        field.value.text().to_string()
    };
    PanelField {
        name: field.name,
        label: field.label,
        required: field.required,
        hint: if choose {
            CHOOSE_HINT.to_string()
        } else {
            field.hint.clone().unwrap_or_default()
        },
        choice,
        choose,
        options: if choice { options(field) } else { Vec::new() },
        left: multiline.then(|| joblog::MAX_NOTES.saturating_sub(value.chars().count())),
        value,
        multiline,
        numeric: field.kind == "number",
    }
}

fn options(field: &Field) -> Vec<PanelOption> {
    if field.kind == "checkbox" {
        let ticked = field.value == Value::Ticked(true);
        return vec![
            option("yes", CHECKBOX_YES, ticked, "tick it".to_string()),
            option("no", CHECKBOX_NO, !ticked, "leave it unticked".to_string()),
        ];
    }
    field
        .options
        .map(|choices| choices.0)
        .unwrap_or_default()
        .iter()
        .map(|&(value, text)| option(value, text, field.value.text() == value, format!("choose \"{text}\"")))
        .collect()
}

fn option(value: &'static str, text: &'static str, chosen: bool, prompt: String) -> PanelOption {
    PanelOption {
        value,
        text,
        chosen,
        prompt,
    }
}

fn file(file: &File) -> PanelFile {
    PanelFile {
        kind: file.kind,
        label: format!("{} (PDF)", file.label),
        ready: file.ready,
        title: if file.ready {
            file.note.unwrap_or("")
        } else {
            file.missing
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logged_entry_says_when() {
        assert_eq!(
            Logged::from(Some(1_760_000_000), "09-10-2025".to_string()),
            Logged {
                checked: true,
                at: "2025-10-09T08:53:20Z".to_string(),
                applied_on: "09-10-2025".to_string(),
            }
        );
        assert!(!Logged::from(None, String::new()).checked);
    }
}
