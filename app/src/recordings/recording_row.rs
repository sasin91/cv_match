//! A recording as the match page shows it: what it is, when, how long, and its transcript.

use super::recording_model::{KIND_NOTE, KIND_VOICE_MESSAGE, Recording};

/// Seconds in a minute.
const SECONDS_PER_MINUTE: i64 = 60;
/// Milliseconds in a second.
const MS_PER_SECOND: i64 = 1000;

/// What the page says while a recording has no transcript yet.
pub const NOT_TRANSCRIBED: &str = "Not transcribed yet.";

#[derive(Debug, Clone, PartialEq)]
pub struct RecordingRow {
    pub id: i64,
    /// "Call, 5 Nov 2026 14:02, 3:41, Danish".
    pub title: String,
    /// The transcript, or NOT_TRANSCRIBED.
    pub transcript: String,
    pub transcribed: bool,
}

impl RecordingRow {
    pub fn from(recording: &Recording) -> Self {
        let mut parts = vec![kind_label(&recording.kind).to_string()];
        parts.push(when(recording.date_created.unwrap_or(recording.upload_date)));
        if let Some(duration_ms) = recording.duration_ms {
            parts.push(minutes_and_seconds(duration_ms));
        }
        if let Some(language) = recording.in_language.as_deref().and_then(language_name) {
            parts.push(language.to_string());
        }
        RecordingRow {
            id: recording.id,
            title: parts.join(", "),
            transcript: recording
                .transcript
                .clone()
                .unwrap_or_else(|| NOT_TRANSCRIBED.to_string()),
            transcribed: recording.transcript.is_some(),
        }
    }
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        KIND_VOICE_MESSAGE => "Voice message",
        KIND_NOTE => "Note",
        _ => "Call",
    }
}

/// unix seconds as a local day and time, "5 Nov 2026 14:02".
fn when(unix_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(unix_seconds, 0)
        .map(|utc| utc.with_timezone(&chrono::Local).format("%-d %b %Y %H:%M").to_string())
        .unwrap_or_default()
}

/// A length as "3:41".
fn minutes_and_seconds(duration_ms: i64) -> String {
    let seconds = duration_ms / MS_PER_SECOND;
    format!("{}:{:02}", seconds / SECONDS_PER_MINUTE, seconds % SECONDS_PER_MINUTE)
}

/// The languages a recording is named by; others aren't named.
fn language_name(code: &str) -> Option<&'static str> {
    match code {
        "da" => Some("Danish"),
        "en" => Some("English"),
        "sv" => Some("Swedish"),
        "no" | "nb" | "nn" => Some("Norwegian"),
        "de" => Some("German"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_reads_as_minutes_and_seconds() {
        assert_eq!(minutes_and_seconds(221_400), "3:41");
        assert_eq!(minutes_and_seconds(5_000), "0:05");
    }

    #[test]
    fn a_recording_not_transcribed_says_so() {
        let recording = Recording {
            id: 1,
            match_id: Some(2),
            kind: "voice_message".to_string(),
            source: "file".to_string(),
            content_url: "a.m4a".to_string(),
            encoding_format: None,
            content_size: None,
            duration_ms: None,
            date_created: None,
            upload_date: 0,
            in_language: None,
            transcript: None,
            summary: None,
            transcribed_by: None,
            transcribed_at: None,
        };
        let row = RecordingRow::from(&recording);
        assert!(row.title.starts_with("Voice message, "));
        assert_eq!(row.transcript, NOT_TRANSCRIBED);
        assert!(!row.transcribed);
    }
}
