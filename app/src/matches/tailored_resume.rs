//! The tailored résumé as fields, ported from modules/cv_match/Tailored_resume.php: a header
//! (name, title, contact, links), intro paragraphs, experience, project and education entries
//! (title, organisation, location, dates, summary, bullets, note), skills and languages, and the
//! section headings in the post's language.
//!
//! tailor_resume asks the model for schema() and passes its answer through clean(); text() writes
//! the plain-text version the page shows and copies, and pdf_writer::resume_pdf lays the fields
//! out. match_model keeps them in match_resumes, match_resume_entries and match_resume_lines.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::php;

/// The headings the model names in the post's language, with the English ones as the fallback.
pub const HEADINGS: [(&str, &str); 5] = [
    ("experience_heading", "Experience"),
    ("skills_heading", "Skills"),
    ("education_heading", "Education"),
    ("languages_heading", "Languages"),
    ("projects_heading", "Projects"),
];

/// The longest each field is kept, in characters: the columns' lengths in MariaDB, which the PHP
/// app cuts to.
mod lengths {
    /// name, title, location, email, and an entry's title, organisation and location.
    pub const LINE: usize = 255;
    pub const PHONE: usize = 64;
    pub const LINK: usize = 300;
    pub const INTRO: usize = 2000;
    /// A skill, a language, a bullet or an entry's note.
    pub const ITEM: usize = 1000;
    pub const EDUCATION_NOTE: usize = 500;
    pub const HEADING: usize = 64;
    /// An entry's starts and ends.
    pub const DATE: usize = 32;
    pub const SUMMARY: usize = 500;
}

/// A tailored résumé's fields, as Cv_match_model::resume_fields gives them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TailoredResume {
    pub name: String,
    pub title: String,
    pub location: String,
    pub phone: String,
    pub email: String,
    pub links: Vec<String>,
    pub intro: Vec<String>,
    pub experience: Vec<ResumeEntry>,
    pub projects: Vec<ResumeEntry>,
    pub skills: Vec<String>,
    pub languages: Vec<String>,
    pub education_note: String,
    pub education: Vec<ResumeEntry>,
    pub experience_heading: String,
    pub skills_heading: String,
    pub education_heading: String,
    pub languages_heading: String,
    pub projects_heading: String,
}

/// A role, project or school.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResumeEntry {
    pub title: String,
    pub organisation: String,
    pub location: String,
    pub starts: String,
    pub ends: String,
    pub summary: String,
    pub bullets: Vec<String>,
    pub note: String,
}

/// The JSON schema the model fills in.
pub fn schema() -> Value {
    let string = json!({ "type": "string" });
    let strings = json!({ "type": "array", "items": string });
    let entry = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "organisation", "location", "starts", "ends", "summary", "bullets", "note"],
        "properties": {
            "title": string,
            "organisation": string,
            "location": string,
            "starts": string,
            "ends": string,
            "summary": string,
            "bullets": strings,
            "note": string,
        },
    });
    let entries = json!({ "type": "array", "items": entry });
    let mut properties = serde_json::Map::new();
    for (name, kind) in [
        ("name", &string),
        ("title", &string),
        ("location", &string),
        ("phone", &string),
        ("email", &string),
        ("links", &strings),
        ("intro", &strings),
        ("experience", &entries),
        ("projects", &entries),
        ("skills", &strings),
        ("languages", &strings),
        ("education_note", &string),
        ("education", &entries),
    ] {
        properties.insert(name.to_string(), kind.clone());
    }
    for (heading, _) in HEADINGS {
        properties.insert(heading.to_string(), string.clone());
    }
    let required: Vec<&String> = properties.keys().collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties,
    })
}

/// The model's answer trimmed and cut to the columns' lengths, with nothing but strings and lists
/// of strings; None when it has no name or no entries at all.
pub fn clean(answer: &Value) -> Option<TailoredResume> {
    if !answer.is_object() {
        return None;
    }
    let field = |name: &str, length: usize| string(&answer[name], length);
    let heading = |name: &str| {
        let english = HEADINGS.iter().find(|(h, _)| *h == name).map(|(_, e)| *e).unwrap_or("");
        let named = field(name, lengths::HEADING);
        if named.is_empty() { english.to_string() } else { named }
    };
    let resume = TailoredResume {
        name: field("name", lengths::LINE),
        title: field("title", lengths::LINE),
        location: field("location", lengths::LINE),
        phone: field("phone", lengths::PHONE),
        email: field("email", lengths::LINE),
        links: strings(&answer["links"], lengths::LINK),
        intro: strings(&answer["intro"], lengths::INTRO),
        experience: entries(&answer["experience"]),
        projects: entries(&answer["projects"]),
        skills: strings(&answer["skills"], lengths::ITEM),
        languages: strings(&answer["languages"], lengths::ITEM),
        education_note: field("education_note", lengths::EDUCATION_NOTE),
        education: entries(&answer["education"]),
        experience_heading: heading("experience_heading"),
        skills_heading: heading("skills_heading"),
        education_heading: heading("education_heading"),
        languages_heading: heading("languages_heading"),
        projects_heading: heading("projects_heading"),
    };
    let no_entries = resume.experience.is_empty() && resume.projects.is_empty() && resume.education.is_empty();
    if resume.name.is_empty() || no_entries {
        return None;
    }
    Some(resume)
}

/// A section's entries, leaving out any without a title.
fn entries(value: &Value) -> Vec<ResumeEntry> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter(|entry| entry.is_object())
        .map(|entry| ResumeEntry {
            title: string(&entry["title"], lengths::LINE),
            organisation: string(&entry["organisation"], lengths::LINE),
            location: string(&entry["location"], lengths::LINE),
            starts: string(&entry["starts"], lengths::DATE),
            ends: string(&entry["ends"], lengths::DATE),
            summary: string(&entry["summary"], lengths::SUMMARY),
            bullets: strings(&entry["bullets"], lengths::ITEM),
            note: string(&entry["note"], lengths::ITEM),
        })
        .filter(|entry| !entry.title.is_empty())
        .collect()
}

/// A string with its whitespace runs as single spaces, trimmed and cut to length characters;
/// anything else is "".
fn string(value: &Value, length: usize) -> String {
    let Some(text) = value.as_str() else {
        return String::new();
    };
    php::trim(&php::collapse_spaces(text)).chars().take(length).collect()
}

/// The strings of a list, each as string() makes it, leaving out the empty ones.
fn strings(value: &Value, length: usize) -> Vec<String> {
    let Some(list) = value.as_array() else {
        return Vec::new();
    };
    list.iter()
        .map(|item| string(item, length))
        .filter(|item| !item.is_empty())
        .collect()
}

/// The résumé as plain text that pastes cleanly: the header lines, the intro, then each section
/// under its heading, entries as "Title · Organisation", their date and place line, and "- "
/// bullets.
pub fn text(resume: &TailoredResume) -> String {
    let links = resume.links.join(" · ");
    let contact = contact(resume);
    let header = join_filled(&[&resume.name, &resume.title, &contact, &links], "\n");
    let mut blocks = vec![header];
    if !resume.intro.is_empty() {
        blocks.push(resume.intro.join("\n\n"));
    }
    for (heading, entries) in [
        (&resume.experience_heading, &resume.experience),
        (&resume.projects_heading, &resume.projects),
    ] {
        if !entries.is_empty() {
            blocks.push(format!("{heading}\n\n{}", entries_text(entries)));
        }
    }
    for (heading, items) in [
        (&resume.skills_heading, &resume.skills),
        (&resume.languages_heading, &resume.languages),
    ] {
        if !items.is_empty() {
            blocks.push(format!("{heading}\n{}", bullets(items)));
        }
    }
    if !resume.education.is_empty() || !resume.education_note.is_empty() {
        let mut parts = vec![resume.education_note.clone()];
        parts.extend(resume.education.iter().map(entry_text));
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        blocks.push(format!(
            "{}\n\n{}",
            resume.education_heading,
            join_filled(&parts, "\n\n")
        ));
    }
    blocks.join("\n\n")
}

fn entries_text(entries: &[ResumeEntry]) -> String {
    entries.iter().map(entry_text).collect::<Vec<_>>().join("\n\n")
}

fn entry_text(entry: &ResumeEntry) -> String {
    let heading = entry_heading(entry);
    let meta = entry_meta(entry);
    let mut text = join_filled(&[&heading, &meta, &entry.summary], "\n");
    if !entry.bullets.is_empty() {
        text.push('\n');
        text.push_str(&bullets(&entry.bullets));
    }
    if !entry.note.is_empty() {
        text.push_str("\n\n");
        text.push_str(&entry.note);
    }
    text
}

fn bullets(items: &[String]) -> String {
    items
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The non-empty parts joined with `separator`.
fn join_filled(parts: &[&str], separator: &str) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(separator)
}

/// "Slagelse, 4200 · +45 50106917 · name@example.com".
pub fn contact(resume: &TailoredResume) -> String {
    join_filled(&[&resume.location, &resume.phone, &resume.email], " · ")
}

/// "Web Developer · JUICE ApS".
pub fn entry_heading(entry: &ResumeEntry) -> String {
    join_filled(&[&entry.title, &entry.organisation], " · ")
}

/// "September 2024 – February 2026 — Copenhagen".
pub fn entry_meta(entry: &ResumeEntry) -> String {
    let dates = join_filled(&[&entry.starts, &entry.ends], " – ");
    join_filled(&[&dates, &entry.location], " — ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_leave_out_what_is_empty() {
        let entry = ResumeEntry {
            title: "Web Developer".into(),
            starts: "2024".into(),
            location: "Copenhagen".into(),
            ..ResumeEntry::default()
        };
        assert_eq!(entry_heading(&entry), "Web Developer");
        assert_eq!(entry_meta(&entry), "2024 — Copenhagen");
        let resume = TailoredResume {
            phone: "+45 50106917".into(),
            email: "name@example.com".into(),
            ..TailoredResume::default()
        };
        assert_eq!(contact(&resume), "+45 50106917 · name@example.com");
    }
}
