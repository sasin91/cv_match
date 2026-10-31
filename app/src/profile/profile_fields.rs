//! The candidate profile's fields on their way in from a CV, ported from
//! modules/profile/Profile_fields.php: schema() is what the model reads a CV into, and from_cv()
//! cleans its answer to the columns, with its "2024-09" months turned into dates.
//!
//! One addition to the PHP: the model also reads the candidate's email, since the desktop app has
//! no account to take it from.

use serde_json::{Value, json};

use super::{LISTS, SECTIONS};

/// Longest name, title, organisation and location, in characters (the VARCHAR(255) columns).
const LINE_LENGTH: usize = 255;
/// Longest phone number, in characters.
const PHONE_LENGTH: usize = 32;
/// Longest link, in characters.
const URL_LENGTH: usize = 512;
/// Longest summary line of an entry, in characters.
const SUMMARY_LENGTH: usize = 500;
/// Longest note, bullet or list item, in characters.
const ITEM_LENGTH: usize = 1000;
/// Longest paragraph of the about text, in characters.
const PARAGRAPH_LENGTH: usize = 2000;

/// The months of the year, for checking a "YYYY-MM" month.
const MONTHS: std::ops::RangeInclusive<u32> = 1..=12;

/// One entry (a job, project or education) as read from the CV.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileEntry {
    pub section: String,
    pub title: String,
    pub organisation: String,
    pub location: String,
    pub url: String,
    /// "YYYY-MM-01", or None.
    pub started_on: Option<String>,
    /// "YYYY-MM-01", or None while it is ongoing.
    pub ended_on: Option<String>,
    pub summary: String,
    pub note: String,
    pub bullets: Vec<String>,
}

/// The profile as read from a CV: the candidate, the header, the entries and the lists.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileFromCv {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub title: String,
    pub location: String,
    pub about: String,
    pub entries: Vec<ProfileEntry>,
    /// kind (LISTS) => its items, in LISTS's order.
    pub lines: Vec<(&'static str, Vec<String>)>,
}

/// The JSON schema the model fills in from a CV.
pub fn schema() -> Value {
    let string = json!({ "type": "string" });
    let strings = json!({ "type": "array", "items": string });
    let sections: Vec<&str> = SECTIONS.iter().map(|(section, _)| *section).collect();
    let entry = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["section", "title", "organisation", "location", "url", "started", "ended", "summary", "bullets", "note"],
        "properties": {
            "section": { "type": "string", "enum": sections },
            "title": string,
            "organisation": string,
            "location": string,
            "url": string,
            "started": string,
            "ended": string,
            "summary": string,
            "bullets": strings,
            "note": string,
        },
    });
    let mut properties = serde_json::Map::new();
    for name in ["name", "email", "phone", "title", "location"] {
        properties.insert(name.to_string(), string.clone());
    }
    properties.insert("about".to_string(), strings.clone());
    properties.insert("entries".to_string(), json!({ "type": "array", "items": entry }));
    for (kind, _) in LISTS {
        properties.insert(list_field(kind), strings.clone());
    }
    let required: Vec<String> = properties.keys().cloned().collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties,
    })
}

/// The answer's field for a list: "skills" for skill.
fn list_field(kind: &str) -> String {
    format!("{kind}s")
}

/// The model's answer as the profile's rows; None when it found no entries at all.
pub fn from_cv(answer: &Value) -> Option<ProfileFromCv> {
    let entries: Vec<ProfileEntry> = answer["entries"]
        .as_array()
        .map(|entries| entries.iter().filter_map(entry).collect())
        .unwrap_or_default();
    if entries.is_empty() {
        return None;
    }
    let lines = LISTS
        .iter()
        .map(|(kind, _)| (*kind, strings(&answer[list_field(kind)], ITEM_LENGTH)))
        .collect();
    Some(ProfileFromCv {
        name: string(&answer["name"], LINE_LENGTH),
        email: string(&answer["email"], LINE_LENGTH),
        phone: string(&answer["phone"], PHONE_LENGTH),
        title: string(&answer["title"], LINE_LENGTH),
        location: string(&answer["location"], LINE_LENGTH),
        about: strings(&answer["about"], PARAGRAPH_LENGTH).join("\n\n"),
        entries,
        lines,
    })
}

/// One entry of the answer, or None when it has no known section or no title.
fn entry(fields: &Value) -> Option<ProfileEntry> {
    let section = fields["section"].as_str().unwrap_or("");
    let known = SECTIONS.iter().any(|(name, _)| *name == section);
    let title = string(&fields["title"], LINE_LENGTH);
    if !known || title.is_empty() {
        return None;
    }
    Some(ProfileEntry {
        section: section.to_string(),
        title,
        organisation: string(&fields["organisation"], LINE_LENGTH),
        location: string(&fields["location"], LINE_LENGTH),
        url: string(&fields["url"], URL_LENGTH),
        started_on: month_date(fields["started"].as_str().unwrap_or("")),
        ended_on: month_date(fields["ended"].as_str().unwrap_or("")),
        summary: string(&fields["summary"], SUMMARY_LENGTH),
        note: string(&fields["note"], ITEM_LENGTH),
        bullets: strings(&fields["bullets"], ITEM_LENGTH),
    })
}

/// "2024-09-01" from "2024-09", "2024-09-15" or "2024"; None for anything else.
pub fn month_date(value: &str) -> Option<String> {
    let value = value.trim();
    let year = value.get(..4).filter(|year| year.chars().all(|c| c.is_ascii_digit()))?;
    let rest = &value[4..];
    let month: u32 = match rest.strip_prefix('-') {
        Some(after) => {
            let digits: String = after.chars().take(2).take_while(char::is_ascii_digit).collect();
            if digits.is_empty() { 1 } else { digits.parse().ok()? }
        }
        None => 1,
    };
    MONTHS.contains(&month).then(|| format!("{year}-{month:02}-01"))
}

/// A string field with its whitespace runs made single spaces, trimmed and cut to length
/// characters; "" for anything but a string.
fn string(value: &Value, length: usize) -> String {
    let Some(text) = value.as_str() else {
        return String::new();
    };
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(length)
        .collect()
}

/// A list field's strings, each as string() makes it, without the empty ones.
fn strings(values: &Value, length: usize) -> Vec<String> {
    values
        .as_array()
        .map(|values| {
            values
                .iter()
                .map(|value| string(value, length))
                .filter(|value| !value.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_month_becomes_its_first_day() {
        assert_eq!(month_date("2024-09").as_deref(), Some("2024-09-01"));
        assert_eq!(month_date("2024-9").as_deref(), Some("2024-09-01"));
        assert_eq!(month_date("2024-09-15").as_deref(), Some("2024-09-01"));
        assert_eq!(month_date("2024").as_deref(), Some("2024-01-01"));
        assert_eq!(month_date(" 2016 ").as_deref(), Some("2016-01-01"));
        assert_eq!(month_date("2024-13"), None);
        assert_eq!(month_date("now"), None);
        assert_eq!(month_date(""), None);
    }

    #[test]
    fn a_cv_is_read_into_entries_with_dates() {
        let answer = json!({
            "name": "  Mette   Sørensen ", "email": "mette@example.com", "phone": "12 34 56 78",
            "title": "Udvikler", "location": "Slagelse, 4200",
            "about": ["Jeg bygger ting.", " ", "Og driver dem."],
            "entries": [
                { "section": "experience", "title": "Udvikler", "organisation": "Firma", "location": "", "url": "",
                  "started": "2017-01", "ended": "", "summary": "Betalinger", "bullets": ["Byggede en API", ""], "note": "" },
                { "section": "hobby", "title": "Klatring", "organisation": "", "location": "", "url": "",
                  "started": "", "ended": "", "summary": "", "bullets": [], "note": "" },
                { "section": "education", "title": "", "organisation": "Zealand", "location": "", "url": "",
                  "started": "", "ended": "2016", "summary": "", "bullets": [], "note": "" },
            ],
            "skills": ["PHP", "Rust"], "languages": ["Dansk"], "certificates": [],
        });
        let profile = from_cv(&answer).unwrap();
        assert_eq!(profile.name, "Mette Sørensen");
        assert_eq!(profile.email, "mette@example.com");
        assert_eq!(profile.about, "Jeg bygger ting.\n\nOg driver dem.");
        assert_eq!(
            profile.entries.len(),
            1,
            "a section it doesn't know, and no title, are left out"
        );
        let entry = &profile.entries[0];
        assert_eq!(entry.started_on.as_deref(), Some("2017-01-01"));
        assert_eq!(entry.ended_on, None);
        assert_eq!(entry.bullets, ["Byggede en API"]);
        assert_eq!(
            profile.lines,
            [
                ("skill", vec!["PHP".to_string(), "Rust".to_string()]),
                ("language", vec!["Dansk".to_string()]),
                ("certificate", vec![]),
            ]
        );
    }

    #[test]
    fn a_cv_without_entries_reads_as_nothing() {
        assert_eq!(from_cv(&json!({ "name": "Mette", "entries": [] })), None);
        assert_eq!(from_cv(&json!("not an object")), None);
    }

    #[test]
    fn the_schema_asks_for_every_field() {
        let schema = schema();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        for field in [
            "name",
            "email",
            "phone",
            "about",
            "entries",
            "skills",
            "languages",
            "certificates",
        ] {
            assert!(required.contains(&field), "{field} in {required:?}");
        }
    }
}
