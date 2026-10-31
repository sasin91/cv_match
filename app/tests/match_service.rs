//! The match page's work answers the same without a web server, as a desktop window will call it:
//! view models and data back, and errors as a kind and a message to show.

use std::sync::{Arc, LazyLock};

use recruiter::account::key_vault::KeyVault;
use recruiter::app_core::AppCore;
use recruiter::db;
use recruiter::matches::match_service::{self, RejectOutcome};
use recruiter::server;
use recruiter::service_error::ErrorKind;
use recruiter::taxonomy::Taxonomy;
use sqlx::Executor;

static TAXONOMY: LazyLock<Arc<Taxonomy>> = LazyLock::new(|| Arc::new(Taxonomy::load()));

/// One saved match, with a résumé written as fields.
const SEED: &str = "
INSERT INTO trongate_users (id) VALUES (3);
INSERT INTO matches (id, trongate_user_id, job_title, company, job_url, job_text, cv_name, cv_text, score, points, max_points, tag, created_at)
VALUES (21, 3, 'PHP-udvikler', 'Firma', 'https://www.jobindex.dk/job/1', 'Vi søger en PHP-udvikler.', 'cv', 'PHP', 0.86, 9, 10, 'top', 1760000000);
INSERT INTO match_items (match_id, criterion, text, verdict, decided_by, reason, sort_order)
VALUES (21, 'requirements', 'PHP', 'met', 'exact', 'Named in the CV.', 0);
";

/// The work's shared state on an in-memory database with SEED in it.
async fn core() -> AppCore {
    let db = db::open_in_memory().await.unwrap();
    db.execute(sqlx::raw_sql(SEED)).await.unwrap();
    let user_id = server::local_user(&db).await.unwrap();
    AppCore::new(db, TAXONOMY.clone(), user_id, KeyVault::in_memory())
}

#[tokio::test]
async fn the_free_match_answers_a_result_to_print() {
    let core = core().await;
    let result = match_service::quick_match(
        &core,
        "PHP-udvikler\nDin profil\n- Erfaring med PHP\n- Kendskab til Docker",
        "Udvikler\nPHP, Laravel, MySQL",
    )
    .await
    .unwrap();
    assert_eq!(result.saved_id, None);
    assert!(result.sections.iter().any(|section| !section.rows.is_empty()));
    assert!(result.cv_found.is_some());
}

#[tokio::test]
async fn the_free_match_says_what_is_missing_from_its_input() {
    let core = core().await;
    let error = match_service::quick_match(&core, "", "PHP").await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Invalid);
    assert_eq!(error.message, "Paste or upload both the CV and the job post.");
}

#[tokio::test]
async fn a_saved_match_is_found_by_id_and_others_are_not() {
    let core = core().await;
    let result = match_service::saved_result(&core, 21).await.unwrap();
    assert_eq!(result.saved_id, Some(21));
    for id in [0, -1, 99] {
        let error = match_service::saved_match(&core, id).await.unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound, "id {id}");
    }
}

#[tokio::test]
async fn a_reject_needs_a_reason_and_can_be_undone() {
    let core = core().await;
    let unfinished = match_service::reject(&core, 21, &[], &[], false).await.unwrap();
    assert!(matches!(unfinished, RejectOutcome::Unfinished(problem) if !problem.is_empty()));
    let reasons = ["commute".to_string()];
    let notes = [("commute".to_string(), "1h each way".to_string())];
    let rejected = match_service::reject(&core, 21, &reasons, &notes, false).await.unwrap();
    assert_eq!(
        rejected,
        RejectOutcome::Done(Some("Rejected. It is in the Rejected list if you change your mind."))
    );
    let back = match_service::reject(&core, 21, &[], &[], true).await.unwrap();
    assert_eq!(back, RejectOutcome::Done(Some("The match is back in your lists.")));
}

#[tokio::test]
async fn a_pdf_is_named_for_the_job_and_needs_a_known_kind() {
    let core = core().await;
    let made = match_service::pdf(&core, 21, "job", "", None).await.unwrap();
    assert!(made.bytes.starts_with(b"%PDF"));
    assert!(made.name.ends_with(".pdf"), "{}", made.name);
    let error = match_service::pdf(&core, 21, "poster", "", None).await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Invalid);
}

#[tokio::test]
async fn a_joblog_status_must_be_one_of_jobnets() {
    let core = core().await;
    let error = match_service::save_joblog_status(&core, 21, "maybe", true)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Malformed);
    assert!(error.message.starts_with("Pick one of Jobnet's statuses:"));
}

#[tokio::test]
async fn link_fetches_stop_after_the_limit() {
    let core = core().await;
    let now = chrono::Utc::now().timestamp();
    for _ in 0..match_service::MAX_FETCHES {
        core.count_fetch(now).unwrap();
    }
    let error = match_service::fetch_job(&core, "https://example.com/job")
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::TooMany);
}

#[tokio::test]
async fn writing_without_a_key_points_to_the_key_page() {
    let core = Arc::new(core().await);
    let error = match_service::write(core, 21, "", "application").await.unwrap_err();
    assert_eq!(error.kind, ErrorKind::NeedKey);
}
