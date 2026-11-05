//! The recordings and recording_segments tables.

use sqlx::SqlitePool;

use crate::whisper::whisper_cli::Heard;

/// What a recording is.
pub const KIND_CALL: &str = "call";
pub const KIND_VOICE_MESSAGE: &str = "voice_message";
pub const KIND_NOTE: &str = "note";

/// Where a recording came from: a file picked on this computer, or one sent from the phone by
/// Taildrop.
pub const SOURCE_FILE: &str = "file";
pub const SOURCE_TAILDROP: &str = "taildrop";

/// A kept recording, as the match page lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub id: i64,
    pub match_id: Option<i64>,
    pub kind: String,
    pub source: String,
    /// The audio file's name in the recordings folder.
    pub content_url: String,
    pub encoding_format: Option<String>,
    pub content_size: Option<i64>,
    pub duration_ms: Option<i64>,
    pub date_created: Option<i64>,
    pub upload_date: i64,
    pub in_language: Option<String>,
    pub transcript: Option<String>,
    /// The short summary (the abstract column).
    pub summary: Option<String>,
    pub transcribed_by: Option<String>,
    pub transcribed_at: Option<i64>,
}

/// A recording to keep, before it has an id or a transcript.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRecording {
    pub match_id: Option<i64>,
    pub kind: &'static str,
    pub source: &'static str,
    pub content_url: String,
    pub encoding_format: Option<String>,
    pub content_size: Option<i64>,
    pub date_created: Option<i64>,
    pub upload_date: i64,
}

/// Whether there is a saved match with id.
pub async fn match_exists(db: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM matches WHERE id = ?1) AS "exists!: bool""#,
        id
    )
    .fetch_one(db)
    .await
}

/// Keeps recording; its id.
pub async fn add(db: &SqlitePool, recording: &NewRecording) -> sqlx::Result<i64> {
    let added = sqlx::query!(
        r#"
        INSERT INTO recordings (match_id, kind, source, content_url, encoding_format, content_size,
                                date_created, upload_date)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        recording.match_id,
        recording.kind,
        recording.source,
        recording.content_url,
        recording.encoding_format,
        recording.content_size,
        recording.date_created,
        recording.upload_date
    )
    .execute(db)
    .await?;
    Ok(added.last_insert_rowid())
}

/// The recording with id, if there is one.
pub async fn find(db: &SqlitePool, id: i64) -> sqlx::Result<Option<Recording>> {
    let row = sqlx::query_as!(
        Recording,
        r#"
        SELECT id AS "id!", match_id, kind, source, content_url, encoding_format, content_size,
               duration_ms, date_created, upload_date, in_language, transcript,
               abstract AS summary, transcribed_by, transcribed_at
        FROM recordings
        WHERE id = ?1
        "#,
        id
    )
    .fetch_optional(db)
    .await?;
    Ok(row)
}

/// The recordings about match_id, newest first.
pub async fn of_match(db: &SqlitePool, match_id: i64) -> sqlx::Result<Vec<Recording>> {
    sqlx::query_as!(
        Recording,
        r#"
        SELECT id AS "id!", match_id, kind, source, content_url, encoding_format, content_size,
               duration_ms, date_created, upload_date, in_language, transcript,
               abstract AS summary, transcribed_by, transcribed_at
        FROM recordings
        WHERE match_id = ?1
        ORDER BY COALESCE(date_created, upload_date) DESC, id DESC
        "#,
        match_id
    )
    .fetch_all(db)
    .await
}

/// Keeps what Whisper heard in recording id: the transcript, its timed lines (replacing any
/// earlier ones), the language, its length, and which model heard it at now.
pub async fn transcribed(
    db: &SqlitePool,
    id: i64,
    heard: &Heard,
    duration_ms: i64,
    transcribed_by: &str,
    now: i64,
) -> sqlx::Result<()> {
    let transcript = heard.transcript();
    let language = (!heard.language.is_empty()).then_some(heard.language.as_str());
    let mut transaction = db.begin().await?;
    sqlx::query!(
        r#"
        UPDATE recordings
        SET transcript = ?2, in_language = ?3, duration_ms = ?4, transcribed_by = ?5, transcribed_at = ?6
        WHERE id = ?1
        "#,
        id,
        transcript,
        language,
        duration_ms,
        transcribed_by,
        now
    )
    .execute(&mut *transaction)
    .await?;
    sqlx::query!(r#"DELETE FROM recording_segments WHERE recording_id = ?1"#, id)
        .execute(&mut *transaction)
        .await?;
    for segment in &heard.segments {
        // Two lines Whisper starts at the same moment are kept as one.
        sqlx::query!(
            r#"
            INSERT INTO recording_segments (recording_id, start_time_ms, end_time_ms, text)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT (recording_id, start_time_ms) DO UPDATE
            SET text = recording_segments.text || ' ' || excluded.text,
                end_time_ms = MAX(recording_segments.end_time_ms, excluded.end_time_ms)
            "#,
            id,
            segment.start_time_ms,
            segment.end_time_ms,
            segment.text
        )
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}

/// Deletes recording id and its timed lines; whether there was one.
pub async fn delete(db: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let deleted = sqlx::query!(r#"DELETE FROM recordings WHERE id = ?1"#, id)
        .execute(db)
        .await?;
    Ok(deleted.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whisper::whisper_cli::Segment;

    fn new_recording(match_id: Option<i64>, upload_date: i64) -> NewRecording {
        NewRecording {
            match_id,
            kind: KIND_CALL,
            source: SOURCE_FILE,
            content_url: format!("{upload_date}.m4a"),
            encoding_format: Some("audio/mp4".to_string()),
            content_size: Some(1000),
            date_created: None,
            upload_date,
        }
    }

    fn segment(start: i64, end: i64, text: &str) -> Segment {
        Segment {
            start_time_ms: start,
            end_time_ms: end,
            text: text.to_string(),
        }
    }

    async fn segments(db: &SqlitePool, id: i64) -> Vec<(i64, i64, String)> {
        sqlx::query_as::<_, (i64, i64, String)>(
            "SELECT start_time_ms, end_time_ms, text FROM recording_segments WHERE recording_id = ?1 ORDER BY start_time_ms",
        )
        .bind(id)
        .fetch_all(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_transcript_is_kept_with_its_timed_lines() {
        let db = crate::db::open_in_memory().await.unwrap();
        let id = add(&db, &new_recording(None, 100)).await.unwrap();
        let heard = Heard {
            language: "da".to_string(),
            segments: vec![
                segment(0, 2000, "Hej."),
                segment(0, 2500, "Det er Mette."),
                segment(3000, 4000, "Kan du komme til samtale?"),
            ],
        };
        transcribed(&db, id, &heard, 4000, "whisper", 200).await.unwrap();
        let recording = find(&db, id).await.unwrap().unwrap();
        assert_eq!(recording.in_language.as_deref(), Some("da"));
        assert_eq!(recording.duration_ms, Some(4000));
        assert_eq!(recording.transcribed_at, Some(200));
        assert!(recording.transcript.unwrap().contains("Kan du komme til samtale?"));
        assert_eq!(
            segments(&db, id).await,
            vec![
                (0, 2500, "Hej. Det er Mette.".to_string()),
                (3000, 4000, "Kan du komme til samtale?".to_string())
            ]
        );
        // Transcribed again, the earlier lines go.
        let again = Heard {
            language: "da".to_string(),
            segments: vec![segment(0, 1000, "Hej igen.")],
        };
        transcribed(&db, id, &again, 1000, "whisper", 300).await.unwrap();
        assert_eq!(segments(&db, id).await, vec![(0, 1000, "Hej igen.".to_string())]);
    }

    #[tokio::test]
    async fn a_deleted_recording_takes_its_lines_along() {
        let db = crate::db::open_in_memory().await.unwrap();
        let id = add(&db, &new_recording(None, 100)).await.unwrap();
        let heard = Heard {
            language: String::new(),
            segments: vec![segment(0, 1000, "Hej.")],
        };
        transcribed(&db, id, &heard, 1000, "whisper", 200).await.unwrap();
        assert!(delete(&db, id).await.unwrap());
        assert!(find(&db, id).await.unwrap().is_none());
        assert!(segments(&db, id).await.is_empty());
        assert!(!delete(&db, id).await.unwrap());
    }
}
