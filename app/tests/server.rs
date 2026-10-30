//! The local server answers the match page's requests as the PHP app does, and only from its
//! own page.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use recruiter::db;
use recruiter::server::{self, AppState};
use recruiter::taxonomy::Taxonomy;
use tower::ServiceExt;

const PORT: u16 = server::DEFAULT_PORT;
const HOST: &str = "127.0.0.1:8765";
const ORIGIN: &str = "http://127.0.0.1:8765";

static TAXONOMY: LazyLock<Arc<Taxonomy>> = LazyLock::new(|| Arc::new(Taxonomy::load()));

/// A saved match with a missed location and an email instruction, and a rejected one.
const SEED: &str = "
INSERT INTO trongate_users (id) VALUES (3);
INSERT INTO cv_matches (id, trongate_user_id, job_title, company, job_url, job_text, cv_name, cv_text, score, points, max_points, tag, created_at)
VALUES (21, 3, 'PHP-udvikler', 'Firma <A/S>', 'https://www.jobindex.dk/job/1', 'Vi søger en PHP-udvikler.\nSend din ansøgning til job@firma.dk. Skriv \"Backend\" i emnefeltet.', 'cv', 'PHP', 0.86, 9, 10, 'top', 1760000000),
       (22, 3, 'Mekaniker', '', NULL, 'Mekaniker søges', 'cv', 'PHP', 0.2, 1, 10, 'poor', 1760000100);
INSERT INTO cv_match_items (cv_match_id, criterion, text, verdict, decided_by, reason, sort_order)
VALUES (21, 'requirements', 'PHP', 'met', 'exact', 'Named in the CV.', 0),
       (21, 'location', 'Bor i København', 'missing', 'llm', 'Lives in Slagelse.', 1);
INSERT INTO cv_match_rejections (cv_match_id, reason, note, created_at) VALUES (22, 'commute', '1h each way', 0);
UPDATE cv_matches SET rejected_at = 1 WHERE id = 22;
";

fn app() -> axum::Router {
    let db = db::open_in_memory().unwrap();
    db.execute_batch(SEED).unwrap();
    let user_id = server::local_user(&db).unwrap();
    server::router(Arc::new(AppState::new(db, TAXONOMY.clone(), user_id, PORT)))
}

async fn send(request: Request<Body>) -> (StatusCode, String) {
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

fn get(path: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap()
}

fn post_json(path: &str, json: &str) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json.to_string()))
        .unwrap()
}

#[tokio::test]
async fn the_page_lists_the_saved_matches_best_first() {
    let (status, page) = send(get("/cv_match")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains(r#"<li class="history-top" id="saved-match-21"><a href="cv_match/21" data-id="21">"#),
        "{page}"
    );
    assert!(page.contains("PHP-udvikler · Firma &#60;A/S&#62;") || page.contains("PHP-udvikler · Firma &lt;A/S&gt;"));
    assert!(page.contains("likely disqualifier: Bor i København"));
    assert!(
        !page.contains("saved-match-22"),
        "rejected matches stay out of the shortlist"
    );
    let (_, rejected) = send(get("/cv_match?list=rejected")).await;
    assert!(rejected.contains("rejected: Too long commute (1h each way)"));
    assert!(rejected.contains(r#"<a href="cv_match?list=rejected" aria-current="page">Rejected</a>"#));
}

#[tokio::test]
async fn a_saved_match_prints_with_its_notices_and_post() {
    let (status, result) = send(get("/cv_match/result/21")).await;
    assert_eq!(status, StatusCode::OK);
    for part in [
        r#"data-saved-id="21""#,
        r#"data-percent="86""#,
        "Likely disqualifier: where you live",
        "Subject line: <code>Backend</code>",
        r#"href="mailto:job@firma.dk?subject=Backend""#,
        r#"action="cv_match/submit_reject/21""#,
        r#"class="document" data-kind="application" data-id="21""#,
        r#"<template class="match-job" data-url="https://www.jobindex.dk/job/1">"#,
    ] {
        assert!(result.contains(part), "{part} in {result}");
    }
    let (status, row) = send(get("/cv_match/result/21?view=row")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(row.contains(r#"<li class="batch-row" data-score="86" data-saved-id="21">"#));
    assert!(row.contains("www.jobindex.dk"));
    let (status, missing) = send(get("/cv_match/result/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing, r#"{"error":"That saved match was not found."}"#);
}

#[tokio::test]
async fn the_free_match_reads_both_texts_and_saves_nothing() {
    let (status, result) = send(post_json(
        "/cv_match/quick_match",
        r#"{"job_text": "PHP-udvikler\nDin profil\n- Erfaring med PHP\n- Kendskab til Docker", "cv_text": "Udvikler\nPHP, Laravel, MySQL"}"#,
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(result.contains(r#"data-saved-id="""#));
    assert!(result.contains(r#"<template class="cv-found""#));
    assert!(!result.contains("submit_reject"));
    let (status, empty) = send(post_json(
        "/cv_match/quick_match",
        r#"{"job_text": "", "cv_text": "x"}"#,
    ))
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(empty, r#"{"error":"Paste or upload both the CV and the job post."}"#);
}

#[tokio::test]
async fn what_isnt_ported_yet_says_so() {
    let (status, answer) = send(post_json("/cv_match/fetch_job", r#"{"url": "https://x.dk"}"#)).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        answer,
        r#"{"error":"Fetching a job post from a link isn't in the desktop app yet."}"#
    );
    let (status, _) = send(get("/cv_match/joblog_panel/21")).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn other_sites_cant_post_here() {
    let request = Request::post("/cv_match/submit_reject/21")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("reasons%5B%5D=salary"))
        .unwrap();
    let (status, _) = send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        Request::get("/cv_match")
            .header(header::HOST, "evil.example")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_page_css_and_js_are_served() {
    for path in [
        "/cv_match_module/css/cv_match.css",
        "/cv_match_module/js/app.js",
        "/js/trongate-mx.min.js",
    ] {
        let response = app().oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}
