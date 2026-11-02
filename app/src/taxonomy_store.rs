//! The taxonomy in the database (migration 0006_taxonomy): the shipped index written into
//! taxonomy_terms and taxonomy_labels whenever the program's copy changes, the labels learned from
//! matches beside it, and the one Taxonomy every lookup uses, built from the active labels.
//!
//! A learned label starts as a candidate, which matching doesn't use. Each match that teaches it
//! again is a sighting; at SIGHTINGS_TO_ACTIVATE it turns active, and the Context's Taxonomy is
//! built again and swapped in whole (reload). The Learned terms screen approves, rejects and
//! renames labels by hand.

use std::collections::{HashMap, HashSet};

use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::context::Context;
use crate::db;
use crate::taxonomy::{self, Label, Taxonomy, TermRecord};

/// The sightings (different matches teaching it) that make a candidate label active.
pub const SIGHTINGS_TO_ACTIVATE: i64 = 2;

/// The first id of a term the app makes itself, above every shipped id.
pub const FIRST_LEARNED_TERM_ID: i64 = 1_000_000;

/// Most labels the Learned terms screen lists, or a search finds.
pub const MAX_LISTED: i64 = 200;

/// Where a label comes from (taxonomy_labels.source).
pub const SHIPPED: &str = "shipped";
pub const LEARNED: &str = "learned";

/// A label's state (taxonomy_labels.status).
pub const ACTIVE: &str = "active";
pub const CANDIDATE: &str = "candidate";
pub const REJECTED: &str = "rejected";

/// What taught a label (taxonomy_label_sightings.learned_from).
pub const FROM_READ: &str = "read";
pub const FROM_JUDGE: &str = "judge";

/// One thing a match taught: text names term_id, in lang.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Lesson {
    pub term_id: i64,
    pub text: String,
    pub lang: &'static str,
    /// FROM_READ or FROM_JUDGE.
    pub learned_from: &'static str,
}

/// A label as the Learned terms screen lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListedLabel {
    pub id: i64,
    pub term_id: i64,
    /// The term's preferred label, Danish first.
    pub term_label: String,
    pub text: String,
    pub lang: String,
    pub source: String,
    pub status: String,
    pub sightings: i64,
    /// The first match that taught it, and when, for a learned label.
    pub first_match_id: Option<i64>,
    pub first_seen_at: Option<i64>,
}

/// The taxonomy for a start: the shipped index written in first when it changed, then the
/// terms with their active labels.
pub async fn open(db: &SqlitePool) -> Result<Taxonomy, String> {
    let now = chrono::Utc::now().timestamp();
    write_shipped_if_changed(db, now)
        .await
        .map_err(|e| format!("The taxonomy couldn't be written to the database: {e}"))?;
    let taxonomy = load(db)
        .await
        .map_err(|e| format!("The taxonomy couldn't be read from the database: {e}"))?;
    reject_unfounded_judge_candidates(db, &taxonomy, now)
        .await
        .map_err(|e| format!("The judge's unfounded labels couldn't be rejected: {e}"))?;
    Ok(taxonomy)
}

/// Rejects the candidate labels only the judge taught whose words share none with their term's
/// labels (Taxonomy::shares_word_stem), as taxonomy_lessons now asks of a judge's lesson: such a
/// label came from a met verdict alone ("SSA til aftenvagt" for Laravel), before that rule. They
/// are rejected rather than deleted, so they aren't learned again and the Learned terms screen
/// still shows them; Approve takes one back.
async fn reject_unfounded_judge_candidates(db: &SqlitePool, taxonomy: &Taxonomy, now: i64) -> sqlx::Result<()> {
    let candidates = sqlx::query!(
        r#"SELECT l.id AS "id!", l.term_id, l.text FROM taxonomy_labels l
           WHERE l.status = 'candidate' AND l.source = 'learned'
             AND EXISTS (SELECT 1 FROM taxonomy_label_sightings s WHERE s.label_id = l.id)
             AND NOT EXISTS (SELECT 1 FROM taxonomy_label_sightings s
                             WHERE s.label_id = l.id AND s.learned_from <> 'judge')"#
    )
    .fetch_all(db)
    .await?;
    let unfounded: Vec<_> = candidates
        .into_iter()
        .filter(|label| !taxonomy.shares_word_stem(&label.text, label.term_id))
        .collect();
    if unfounded.is_empty() {
        return Ok(());
    }
    let mut tx = db::write_transaction(db).await?;
    for label in &unfounded {
        set_status(&mut tx, label.id, REJECTED, now).await?;
    }
    tx.commit().await?;
    for label in &unfounded {
        crate::log_line!(
            "taxonomy: rejected \"{}\" for term {}: only the judge taught it, and it shares no word with the term",
            label.text,
            label.term_id
        );
    }
    Ok(())
}

/// The taxonomy built again from the database and swapped into context, after a label turned
/// active, was approved, rejected or renamed.
pub async fn reload(context: &Context) -> Result<(), String> {
    let taxonomy = load(context.db())
        .await
        .map_err(|e| format!("The taxonomy couldn't be read again: {e}"))?;
    context.replace_taxonomy(taxonomy);
    Ok(())
}

/// The shipped index's sha256, as lowercase hex.
fn shipped_sha256() -> String {
    Sha256::digest(taxonomy::INDEX_JSON.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes the shipped index into the tables in one transaction when its sha256 differs from the
/// one last written: terms upserted, terms the index no longer has removed (with the labels
/// learned for them, logged), every shipped label written again. Learned labels stay; one that
/// replaced or rejected a shipped label keeps that label's text from coming back.
async fn write_shipped_if_changed(db: &SqlitePool, now: i64) -> sqlx::Result<()> {
    let sha256 = shipped_sha256();
    let written = sqlx::query_scalar!(r#"SELECT index_sha256 FROM taxonomy_shipped WHERE id = 1"#)
        .fetch_optional(db)
        .await?;
    if written.as_deref() == Some(sha256.as_str()) {
        return Ok(());
    }
    let records = taxonomy::shipped_terms().map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let mut tx = db::write_transaction(db).await?;
    for record in &records {
        sqlx::query!(
            r#"INSERT INTO taxonomy_terms (id, kind, parent_id, definition, source)
               VALUES (?1, ?2, ?3, ?4, 'shipped')
               ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, parent_id = excluded.parent_id,
                 definition = excluded.definition, source = 'shipped'"#,
            record.id,
            record.kind,
            record.parent_id,
            record.definition,
        )
        .execute(&mut *tx)
        .await?;
    }
    let kept: HashSet<i64> = records.iter().map(|record| record.id).collect();
    let shipped_ids = sqlx::query_scalar!(r#"SELECT id FROM taxonomy_terms WHERE source = 'shipped'"#)
        .fetch_all(&mut *tx)
        .await?;
    for gone in shipped_ids.into_iter().filter(|id| !kept.contains(id)) {
        let learned = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM taxonomy_labels WHERE term_id = ?1 AND source = 'learned'"#,
            gone
        )
        .fetch_one(&mut *tx)
        .await?;
        if learned > 0 {
            crate::log_line!("taxonomy: term {gone} left the shipped index, with its {learned} learned labels");
        }
        sqlx::query!(r#"DELETE FROM taxonomy_terms WHERE id = ?1"#, gone)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query!(r#"DELETE FROM taxonomy_labels WHERE source = 'shipped'"#)
        .execute(&mut *tx)
        .await?;
    let mut labels = 0;
    for record in &records {
        for label in &record.labels {
            sqlx::query_file!(
                "queries/taxonomy_store/write_shipped_label.sql",
                record.id,
                label.text,
                label.lang,
                label.preferred,
                now,
            )
            .execute(&mut *tx)
            .await?;
            labels += 1;
        }
    }
    sqlx::query!(
        r#"INSERT INTO taxonomy_shipped (id, index_sha256, written_at) VALUES (1, ?1, ?2)
           ON CONFLICT (id) DO UPDATE SET index_sha256 = excluded.index_sha256, written_at = excluded.written_at"#,
        sha256,
        now,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::log_line!(
        "taxonomy: wrote the shipped index ({} terms, {labels} labels)",
        records.len()
    );
    Ok(())
}

/// The terms with their active labels, as one Taxonomy: terms by id, and each term's shipped
/// labels in the index's order before its learned ones, so a database without learned labels
/// gives exactly the shipped index's Taxonomy.
pub async fn load(db: &SqlitePool) -> sqlx::Result<Taxonomy> {
    let terms = sqlx::query!(r#"SELECT id, kind, parent_id, definition FROM taxonomy_terms ORDER BY id"#)
        .fetch_all(db)
        .await?;
    let labels = sqlx::query!(
        r#"SELECT term_id, text, lang, preferred AS "preferred: bool" FROM taxonomy_labels
           WHERE status = 'active' ORDER BY term_id, source <> 'shipped', id"#
    )
    .fetch_all(db)
    .await?;
    let mut by_term: HashMap<i64, Vec<Label>> = HashMap::new();
    for label in labels {
        by_term.entry(label.term_id).or_default().push(Label {
            text: label.text,
            lang: label.lang,
            preferred: label.preferred,
        });
    }
    let records = terms
        .into_iter()
        .map(|term| TermRecord {
            labels: by_term.remove(&term.id).unwrap_or_default(),
            id: term.id,
            kind: term.kind,
            parent_id: term.parent_id,
            definition: term.definition,
        })
        .collect();
    Ok(Taxonomy::from_records(records))
}

/// Records what match_id taught (answered by model): each lesson's label made a candidate when
/// it is new, and given the match as a sighting. Returns the labels that turned active, so the
/// caller reloads the taxonomy. A lesson is skipped when its text is already another term's
/// active label, or its label was rejected.
pub async fn learn(
    db: &SqlitePool,
    lessons: &[Lesson],
    match_id: i64,
    model: &str,
    now: i64,
) -> sqlx::Result<Vec<String>> {
    let mut activated = Vec::new();
    let mut tx = db::write_transaction(db).await?;
    for lesson in lessons {
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM taxonomy_labels
                 WHERE text = ?1 COLLATE NOCASE AND lang = ?2 AND status = 'active' AND term_id <> ?3) AS "taken: bool""#,
            lesson.text,
            lesson.lang,
            lesson.term_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if taken {
            continue;
        }
        sqlx::query!(
            r#"INSERT INTO taxonomy_labels (term_id, text, lang, preferred, source, status, created_at, updated_at)
               VALUES (?1, ?2, ?3, 0, 'learned', 'candidate', ?4, ?4)
               ON CONFLICT (term_id, text, lang) DO NOTHING"#,
            lesson.term_id,
            lesson.text,
            lesson.lang,
            now,
        )
        .execute(&mut *tx)
        .await?;
        let label = sqlx::query!(
            r#"SELECT id, status, created_at FROM taxonomy_labels WHERE term_id = ?1 AND text = ?2 AND lang = ?3"#,
            lesson.term_id,
            lesson.text,
            lesson.lang,
        )
        .fetch_one(&mut *tx)
        .await?;
        if label.status != CANDIDATE {
            continue;
        }
        if label.created_at == now {
            crate::log_line!(
                "taxonomy: learned \"{}\" for term {} from match {match_id} ({})",
                lesson.text,
                lesson.term_id,
                lesson.learned_from
            );
        }
        sqlx::query!(
            r#"INSERT INTO taxonomy_label_sightings (label_id, match_id, learned_from, model, seen_at)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT (label_id, match_id) DO NOTHING"#,
            label.id,
            match_id,
            lesson.learned_from,
            model,
            now,
        )
        .execute(&mut *tx)
        .await?;
        let sightings = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM taxonomy_label_sightings WHERE label_id = ?1"#,
            label.id
        )
        .fetch_one(&mut *tx)
        .await?;
        if sightings >= SIGHTINGS_TO_ACTIVATE {
            set_status(&mut tx, label.id, ACTIVE, now).await?;
            crate::log_line!(
                "taxonomy: \"{}\" is now active for term {} ({sightings} matches taught it)",
                lesson.text,
                lesson.term_id
            );
            activated.push(lesson.text.clone());
        }
    }
    tx.commit().await?;
    Ok(activated)
}

async fn set_status(tx: &mut sqlx::SqliteConnection, id: i64, status: &str, now: i64) -> sqlx::Result<()> {
    sqlx::query!(
        r#"UPDATE taxonomy_labels SET status = ?2, updated_at = ?3 WHERE id = ?1"#,
        id,
        status,
        now
    )
    .execute(tx)
    .await?;
    Ok(())
}

/// The learned labels, candidates first, newest first, at most MAX_LISTED.
pub async fn learned_labels(db: &SqlitePool) -> sqlx::Result<Vec<ListedLabel>> {
    let rows = sqlx::query_file_as!(ListedLabel, "queries/taxonomy_store/learned_labels.sql", MAX_LISTED)
        .fetch_all(db)
        .await?;
    Ok(rows)
}

/// A match that taught a label: where it was found and why.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelSighting {
    pub match_id: i64,
    /// FROM_READ or FROM_JUDGE.
    pub learned_from: String,
    /// The provider/model that answered.
    pub model: String,
    pub seen_at: i64,
    pub job_title: String,
    pub company: String,
    pub job_url: Option<String>,
    /// The match's item for the requirement the label is, when it is found: its verdict, who
    /// decided it, and the judge's quote of the CV and reason.
    pub verdict: Option<String>,
    pub decided_by: Option<String>,
    pub evidence: Option<String>,
    pub reason: Option<String>,
}

/// The matches that taught label id, oldest first.
pub async fn sightings(db: &SqlitePool, id: i64) -> sqlx::Result<Vec<LabelSighting>> {
    sqlx::query_file_as!(LabelSighting, "queries/taxonomy_store/label_sightings.sql", id)
        .fetch_all(db)
        .await
}

/// Every label, shipped or learned, whose text contains words (case aside), at most MAX_LISTED,
/// for editing a shipped label too.
pub async fn find_labels(db: &SqlitePool, words: &str) -> sqlx::Result<Vec<ListedLabel>> {
    let pattern = format!("%{}%", like_escaped(words.trim()));
    let rows = sqlx::query_file_as!(
        ListedLabel,
        "queries/taxonomy_store/find_labels.sql",
        pattern,
        MAX_LISTED
    )
    .fetch_all(db)
    .await?;
    Ok(rows)
}

/// text with LIKE's wildcards and its escape character taken literally (ESCAPE '\').
fn like_escaped(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// A label turned active by hand.
pub async fn approve(db: &SqlitePool, id: i64, now: i64) -> sqlx::Result<()> {
    let mut tx = db::write_transaction(db).await?;
    set_status(&mut tx, id, ACTIVE, now).await?;
    tx.commit().await?;
    crate::log_line!("taxonomy: label {id} approved");
    Ok(())
}

/// A label rejected: kept as rejected, so it isn't learned again; a shipped one becomes learned,
/// so writing the shipped index again doesn't bring it back.
pub async fn reject(db: &SqlitePool, id: i64, now: i64) -> sqlx::Result<()> {
    let mut tx = db::write_transaction(db).await?;
    sqlx::query!(
        r#"UPDATE taxonomy_labels SET status = 'rejected', source = 'learned', preferred = 0, updated_at = ?2 WHERE id = ?1"#,
        id,
        now
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::log_line!("taxonomy: label {id} rejected");
    Ok(())
}

/// A label's text changed to text. An active or candidate learned label is changed where it is;
/// a shipped or rejected one is kept rejected (so the shipped index doesn't bring its text back)
/// and its term gets text as a new active learned label. Err when the term already has that label.
pub async fn rename(db: &SqlitePool, id: i64, text: &str, now: i64) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("A label can't be empty.".to_string());
    }
    rename_in_database(db, id, text, now).await.map_err(|e| {
        crate::log_line!("taxonomy: renaming label {id} failed: {e}");
        if is_unique_violation(&e) {
            "That term already has this label.".to_string()
        } else {
            format!("The label couldn't be changed: {e}")
        }
    })?;
    crate::log_line!("taxonomy: label {id} renamed to \"{text}\"");
    Ok(())
}

async fn rename_in_database(db: &SqlitePool, id: i64, text: &str, now: i64) -> sqlx::Result<()> {
    let mut tx = db::write_transaction(db).await?;
    let label = sqlx::query!(
        r#"SELECT term_id, lang, source, status FROM taxonomy_labels WHERE id = ?1"#,
        id
    )
    .fetch_one(&mut *tx)
    .await?;
    // A shipped label, or a rejected one (which may be a rejected shipped label's tombstone),
    // stays as a rejected row with its text, so writing the shipped index again can't bring the
    // text back; the new text is a label of its own.
    if label.source == SHIPPED || label.status == REJECTED {
        sqlx::query!(
            r#"UPDATE taxonomy_labels SET status = 'rejected', source = 'learned', preferred = 0, updated_at = ?2 WHERE id = ?1"#,
            id,
            now
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"INSERT INTO taxonomy_labels (term_id, text, lang, preferred, source, status, created_at, updated_at)
               VALUES (?1, ?2, ?3, 0, 'learned', 'active', ?4, ?4)"#,
            label.term_id,
            text,
            label.lang,
            now,
        )
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query!(
            r#"UPDATE taxonomy_labels SET text = ?2, updated_at = ?3 WHERE id = ?1"#,
            id,
            text,
            now
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// What the Learned terms screen does to a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Review {
    Approve,
    Reject,
    Rename(String),
}

/// A review of label id done and the taxonomy built again with it, and what to say; Err says why
/// it couldn't be done (logged).
pub async fn review(context: &Context, id: i64, review: Review) -> Result<String, String> {
    let now = chrono::Utc::now().timestamp();
    let db = context.db();
    let said = match &review {
        Review::Approve => approve(db, id, now).await.map(|()| "Approved: matching uses it now."),
        Review::Reject => reject(db, id, now)
            .await
            .map(|()| "Rejected: it won't be learned again."),
        Review::Rename(text) => return rename_and_reload(context, id, text, now).await,
    }
    .map_err(|e| {
        crate::log_line!("taxonomy: {review:?} of label {id} failed: {e}");
        format!("The label couldn't be changed: {e}")
    })?;
    reload(context).await?;
    Ok(said.to_string())
}

async fn rename_and_reload(context: &Context, id: i64, text: &str, now: i64) -> Result<String, String> {
    rename(context.db(), id, text, now).await?;
    reload(context).await?;
    Ok("Saved: matching uses the new label now.".to_string())
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|e| e.kind() == sqlx::error::ErrorKind::UniqueViolation)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn opened() -> (SqlitePool, Taxonomy) {
        let db = db::open_in_memory().await.unwrap();
        let taxonomy = open(&db).await.unwrap();
        (db, taxonomy)
    }

    /// A saved match to sight a label in.
    async fn a_match(db: &SqlitePool, job_text: &str) -> i64 {
        let user_id = crate::server::local_user(db).await.unwrap();
        sqlx::query_scalar(
            r#"INSERT INTO matches (trongate_user_id, job_title, job_text, score, points, max_points, tag, created_at)
               VALUES (?1, 'job', ?2, 0, 0, 0, '', 0) RETURNING id"#,
        )
        .bind(user_id)
        .bind(job_text)
        .fetch_one(db)
        .await
        .unwrap()
    }

    fn records(taxonomy: &Taxonomy) -> Vec<TermRecord> {
        taxonomy
            .terms
            .iter()
            .map(|term| TermRecord {
                id: term.id,
                kind: term.kind.clone(),
                parent_id: term.parent_id,
                labels: term.labels.clone(),
                definition: term.definition.clone(),
            })
            .collect()
    }

    #[tokio::test]
    async fn the_database_gives_the_shipped_index_exactly() {
        let (db, taxonomy) = opened().await;
        assert_eq!(records(&taxonomy), taxonomy::shipped_terms().unwrap());
        // A second start finds the same index written and writes nothing.
        let written: i64 = sqlx::query_scalar("SELECT written_at FROM taxonomy_shipped")
            .fetch_one(&db)
            .await
            .unwrap();
        sqlx::query("UPDATE taxonomy_shipped SET written_at = 1")
            .execute(&db)
            .await
            .unwrap();
        open(&db).await.unwrap();
        let again: i64 = sqlx::query_scalar("SELECT written_at FROM taxonomy_shipped")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!((again, written > 1), (1, true));
    }

    #[tokio::test]
    async fn a_label_turns_active_at_its_second_match_and_survives_a_new_index() {
        let (db, taxonomy) = opened().await;
        let term = &taxonomy.terms[0];
        let lesson = Lesson {
            term_id: term.id,
            text: "Helt ny etiket".to_string(),
            lang: "da",
            learned_from: FROM_READ,
        };
        let first = a_match(&db, "post one").await;
        let second = a_match(&db, "post two").await;
        assert!(
            learn(&db, std::slice::from_ref(&lesson), first, "llama_cpp/x", 10)
                .await
                .unwrap()
                .is_empty()
        );
        // The same match again is no second sighting.
        assert!(
            learn(&db, std::slice::from_ref(&lesson), first, "llama_cpp/x", 11)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !load(&db).await.unwrap().terms[0]
                .labels
                .iter()
                .any(|l| l.text == lesson.text)
        );
        assert_eq!(
            learn(&db, std::slice::from_ref(&lesson), second, "llama_cpp/x", 12)
                .await
                .unwrap(),
            vec![lesson.text.clone()]
        );
        let learned = &load(&db).await.unwrap().terms[0].labels;
        assert_eq!(
            learned.last().map(|l| (l.text.as_str(), l.preferred)),
            Some(("Helt ny etiket", false))
        );
        // A changed shipped index is written again; the learned label stays.
        sqlx::query("UPDATE taxonomy_shipped SET index_sha256 = 'old'")
            .execute(&db)
            .await
            .unwrap();
        let reopened = open(&db).await.unwrap();
        assert!(reopened.terms[0].labels.iter().any(|l| l.text == lesson.text));
        let listed = learned_labels(&db).await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|l| (l.text.as_str(), l.status.as_str(), l.sightings, l.first_match_id))
                .collect::<Vec<_>>(),
            [("Helt ny etiket", ACTIVE, 2, Some(first))]
        );
    }

    #[tokio::test]
    async fn a_rejected_shipped_label_stays_rejected_after_a_new_index() {
        let (db, taxonomy) = opened().await;
        let term = &taxonomy.terms[0];
        let label = term.labels[0].text.clone();
        let found = find_labels(&db, &label).await.unwrap();
        let shipped = found.iter().find(|l| l.term_id == term.id && l.text == label).unwrap();
        rename(&db, shipped.id, "Et bedre navn", 5).await.unwrap();
        sqlx::query("UPDATE taxonomy_shipped SET index_sha256 = 'old'")
            .execute(&db)
            .await
            .unwrap();
        let reopened = open(&db).await.unwrap();
        let labels: Vec<&str> = reopened.terms[0].labels.iter().map(|l| l.text.as_str()).collect();
        assert!(!labels.contains(&label.as_str()), "{labels:?}");
        assert!(labels.contains(&"Et bedre navn"), "{labels:?}");
    }

    #[tokio::test]
    async fn editing_a_rejected_shipped_label_keeps_it_out_and_the_edit_active() {
        let (db, taxonomy) = opened().await;
        let term = &taxonomy.terms[0];
        let label = term.labels[0].text.clone();
        let found = find_labels(&db, &label).await.unwrap();
        let shipped = found.iter().find(|l| l.term_id == term.id && l.text == label).unwrap();
        reject(&db, shipped.id, 5).await.unwrap();
        rename(&db, shipped.id, "Et rettet navn", 6).await.unwrap();
        sqlx::query("UPDATE taxonomy_shipped SET index_sha256 = 'old'")
            .execute(&db)
            .await
            .unwrap();
        let reopened = open(&db).await.unwrap();
        let labels: Vec<&str> = reopened.terms[0].labels.iter().map(|l| l.text.as_str()).collect();
        assert!(!labels.contains(&label.as_str()), "{labels:?}");
        assert!(labels.contains(&"Et rettet navn"), "{labels:?}");
    }

    #[tokio::test]
    async fn another_terms_label_is_not_learned() {
        let (db, taxonomy) = opened().await;
        let other = taxonomy.terms[1].labels[0].text.clone();
        let lesson = Lesson {
            term_id: taxonomy.terms[0].id,
            text: other.to_uppercase(),
            lang: if taxonomy.terms[1].labels[0].lang == "en" {
                "en"
            } else {
                "da"
            },
            learned_from: FROM_JUDGE,
        };
        let one = a_match(&db, "a").await;
        learn(&db, &[lesson], one, "", 1).await.unwrap();
        assert!(learned_labels(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_start_rejects_judge_candidates_that_share_no_word_with_their_term() {
        let (db, taxonomy) = opened().await;
        let laravel = taxonomy
            .terms
            .iter()
            .find(|term| term.labels.iter().any(|l| l.text == "Laravel"))
            .unwrap()
            .id;
        let lesson = |text: &str, learned_from: &'static str| Lesson {
            term_id: laravel,
            text: text.to_string(),
            lang: "da",
            learned_from,
        };
        let one = a_match(&db, "a").await;
        let lessons = [
            lesson("SSA til aftenvagt", FROM_JUDGE),
            lesson("Laravel-udvikling", FROM_JUDGE),
            lesson("Backend på vores webshop", FROM_READ),
        ];
        learn(&db, &lessons, one, "", 1).await.unwrap();
        open(&db).await.unwrap();
        let statuses: Vec<(String, String)> = learned_labels(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|l| (l.text, l.status))
            .collect();
        let status_of = |text: &str| {
            statuses
                .iter()
                .find(|(label, _)| label == text)
                .map(|(_, status)| status.as_str())
        };
        assert_eq!(status_of("SSA til aftenvagt"), Some(REJECTED));
        assert_eq!(status_of("Laravel-udvikling"), Some(CANDIDATE));
        // The model's read of the post named the term in English: not the judge's to undo.
        assert_eq!(status_of("Backend på vores webshop"), Some(CANDIDATE));
    }

    #[test]
    fn like_wildcards_are_taken_literally() {
        assert_eq!(like_escaped(r"50%_a\b"), r"50\%\_a\\b");
    }
}
