//! The tailored résumé as fields, ported from modules/cv_match/Tailored_resume.php: a header
//! (name, title, contact, links), intro paragraphs, experience, project and education entries
//! (title, organisation, location, dates, summary, bullets, note), skills and languages, and the
//! section headings in the post's language.
//!
//! So far only what Pdf_writer::resume_pdf lays out: the fields as cv_match_model::resume_fields
//! reads them, and the contact, entry heading and entry meta lines. schema(), clean() and text()
//! come with tailor_resume.

use serde::{Deserialize, Serialize};

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
