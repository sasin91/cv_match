//! The local server answers the match page's requests as the PHP app does, and only from its
//! own page.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use recruiter::db;
use recruiter::server::{self, AppState};
use recruiter::taxonomy::Taxonomy;
use sqlx::{Executor, SqlitePool};
use tower::ServiceExt;

const PORT: u16 = server::DEFAULT_PORT;
const HOST: &str = "127.0.0.1:8765";
const ORIGIN: &str = "http://127.0.0.1:8765";

static TAXONOMY: LazyLock<Arc<Taxonomy>> = LazyLock::new(|| Arc::new(Taxonomy::load()));

/// A saved match with a missed location, an email instruction and a résumé written as fields, and
/// a rejected one.
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
UPDATE cv_matches SET resume_text = 'Mette Sørensen' WHERE id = 21;
INSERT INTO cv_match_resumes (cv_match_id, name, intro) VALUES (21, 'Mette Sørensen', 'Backend-udvikler.');
INSERT INTO cv_match_resume_entries (id, cv_match_id, section, title, sort_order) VALUES (1, 21, 'experience', 'Udvikler', 0);
INSERT INTO cv_match_resume_lines (cv_match_id, entry_id, kind, text, sort_order)
VALUES (21, 1, 'bullet', 'Byggede en API', 0), (21, NULL, 'skill', 'PHP', 1);
";

/// An in-memory database with SEED in it.
async fn seeded() -> SqlitePool {
    let db = db::open_in_memory().await.unwrap();
    db.execute(sqlx::raw_sql(SEED)).await.unwrap();
    db
}

async fn app() -> axum::Router {
    let db = seeded().await;
    let user_id = server::local_user(&db).await.unwrap();
    server::router(Arc::new(AppState::new(db, TAXONOMY.clone(), user_id, PORT)))
}

async fn send(request: Request<Body>) -> (StatusCode, String) {
    send_to(&app().await, request).await
}

/// Sends to one app, so a later request sees what an earlier one saved.
async fn send_to(app: &axum::Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.clone().oneshot(request).await.unwrap();
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
    let (status, _) = send(get("/cv_match/application_delivery/21")).await;
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
        let response = app().await.oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn pdf_js_is_served_from_the_app_for_offline_uploads() {
    for path in [
        "/cv_match_module/pdfjs/pdf.min.mjs",
        "/cv_match_module/pdfjs/pdf.worker.min.mjs",
    ] {
        let response = app().await.oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        // Browsers only run a module whose type is JavaScript.
        let mime = response.headers()[header::CONTENT_TYPE].to_str().unwrap();
        assert!(mime.contains("javascript"), "{path}: {mime}");
    }
}

/// A saved match's PDF: its status, Content-Type and Content-Disposition, and its bytes.
async fn pdf(app: &axum::Router, json: &str) -> (StatusCode, String, String, Vec<u8>) {
    let response = app.clone().oneshot(post_json("/cv_match/pdf", json)).await.unwrap();
    let status = response.status();
    let header_of = |name| {
        response
            .headers()
            .get(name)
            .map(|value: &header::HeaderValue| value.to_str().unwrap().to_string())
            .unwrap_or_default()
    };
    let (mime, disposition) = (header_of(header::CONTENT_TYPE), header_of(header::CONTENT_DISPOSITION));
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, mime, disposition, body)
}

#[tokio::test]
async fn a_saved_match_downloads_as_a_pdf_named_for_the_job() {
    let app = app().await;
    let (status, mime, disposition, body) = pdf(
        &app,
        r#"{"id": "21", "kind": "application", "text": "Kære Firma\n\nJeg søger."}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(mime, "application/pdf");
    assert_eq!(
        disposition,
        "attachment; filename=\"Application - PHP-udvikler - Firma A S.pdf\"; filename*=UTF-8''Application%20-%20PHP-udvikler%20-%20Firma%20A%20S.pdf"
    );
    assert!(body.starts_with(b"%PDF-"));

    // The job post and the CV are always the saved text; the job post's name is in Danish too.
    let (status, _, disposition, _) = pdf(&app, r#"{"id": 22, "kind": "job"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        disposition.contains("filename*=UTF-8''Job%20ad%20-%20Mekaniker.pdf"),
        "{disposition}"
    );
    let (status, _, _, _) = pdf(&app, r#"{"id": 21, "kind": "cv", "text": "ignored"}"#).await;
    assert_eq!(status, StatusCode::OK);

    // A résumé written as fields is laid out from them.
    let (status, _, disposition, body) = pdf(&app, r#"{"id": 21, "kind": "resume"}"#).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(disposition.contains("Resume%20-%20PHP-udvikler"));
}

#[tokio::test]
async fn a_pdf_without_text_or_kind_says_why() {
    let app = app().await;
    for (json, status, error) in [
        (
            r#"{"id": 22, "kind": "application"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "There is no text to export yet.",
        ),
        (
            r#"{"id": 21, "kind": "letter"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
            "Choose the application, the résumé, the CV or the job post.",
        ),
        (
            r#"{"id": 99, "kind": "job"}"#,
            StatusCode::NOT_FOUND,
            "That saved match was not found.",
        ),
    ] {
        let (answer_status, mime, _, body) = pdf(&app, json).await;
        assert_eq!(answer_status, status, "{json}");
        assert_eq!(mime, "application/json");
        let answer: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(answer["error"], error, "{json}");
    }
}

/// The Jobnet extension's origin (extension::ID).
const EXTENSION: &str = "chrome-extension://jijhihfppobcoconjjllmffbohpffbll";

#[tokio::test]
async fn the_joblog_is_filled_in_from_the_saved_match() {
    let (status, panel) = send(get("/cv_match/joblog_panel/21")).await;
    assert_eq!(status, StatusCode::OK, "{panel}");
    assert!(
        panel
            .contains(r#"<div class="joblog-field" data-name="occupation" data-label="Stilling eller arbejdsområde">"#)
    );
    assert!(panel.contains(r#"<input type="text" value="job@firma.dk" inputmode="text" aria-label="E-mail">"#));
    assert!(panel.contains(r#"<button type="button" class="chip chosen" aria-pressed="true" data-value="IkkeSoegt" data-prompt="choose &#34;Ikke søgt&#34;">Ikke søgt</button>"#)
        || panel.contains(r#"data-prompt="choose &quot;Ikke søgt&quot;">Ikke søgt</button>"#), "{panel}");
    assert!(panel.contains("Jobansøgning: Skriv ansøgningen først (Write application)."));
    assert!(panel.contains(r#"<label class="joblog-logged"><input type="checkbox"> I&#39;ve entered this job in my joblog on Jobnet</label>"#)
        || panel.contains("<input type=\"checkbox\"> I&#x27;ve entered"), "{panel}");

    let (status, entry) = send(get("/cv_match/joblog/21")).await;
    assert_eq!(status, StatusCode::OK);
    let entry: serde_json::Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(entry["sections"][0]["fields"][0]["value"], "PHP-udvikler");
    assert_eq!(entry["sections"][1]["fields"][3]["value"], true);
    assert_eq!(entry["logged_at"], serde_json::Value::Null);

    let (status, answer) = send(get("/cv_match/joblog/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(answer, r#"{"error":"That saved match was not found."}"#);
}

#[tokio::test]
async fn the_extension_marks_a_match_as_on_jobnet() {
    let app = app().await;
    let (_, history) = send_to(&app, get("/cv_match/history")).await;
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    assert_eq!(history["matches"][0]["id"], 21);
    assert_eq!(history["matches"][0]["job_title"], "PHP-udvikler");
    assert_eq!(
        history["matches"].as_array().unwrap().len(),
        1,
        "rejected matches stay out"
    );
    assert_eq!(history["not_joblogged"], serde_json::json!([]));

    // The page says it was applied for: it is waiting to be logged.
    let (status, logged) = send_to(
        &app,
        post_json(
            "/cv_match/joblog_status",
            r#"{"id": "21", "status": "Soegt", "logged": false}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{logged}");
    assert!(logged.contains(r#"<section class="joblog-section joblog-saved" data-applied-on=""#));
    assert!(!logged.contains("<time"));
    let (_, history) = send_to(&app, get("/cv_match/history")).await;
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    assert_eq!(history["not_joblogged"][0]["id"], 21);
    assert_eq!(history["not_joblogged"][0]["jobnet_status"], "Soegt");

    // The extension saved it on Jobnet.
    let request = Request::post("/cv_match/joblog_status")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, EXTENSION)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"id": 21, "status": "Samtale", "logged": true}"#))
        .unwrap();
    let (status, logged) = send_to(&app, request).await;
    assert_eq!(status, StatusCode::OK, "{logged}");
    assert!(logged.contains(" checked>"));
    assert!(logged.contains("Entered on <time datetime=\""));
    let (_, history) = send_to(&app, get("/cv_match/history")).await;
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    assert_eq!(history["not_joblogged"], serde_json::json!([]));
    let (_, entry) = send_to(&app, get("/cv_match/joblog/21")).await;
    let entry: serde_json::Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(entry["sections"][2]["fields"][0]["value"], "Samtale");
    assert!(entry["logged_at"].is_i64());
    // The day applied is kept from Søgt.
    assert_eq!(
        entry["sections"][2]["fields"][1]["value"].as_str().unwrap().len(),
        "30-10-2026".len()
    );

    let (status, answer) = send_to(
        &app,
        post_json("/cv_match/joblog_status", r#"{"id": 21, "status": "Done"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        answer,
        r#"{"error":"Pick one of Jobnet's statuses: Ikke søgt, Søgt, Samtale."}"#
    );
    let (status, _) = send_to(
        &app,
        post_json("/cv_match/joblog_status", r#"{"id": 22000, "status": "Soegt"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_extension_setup_page_says_where_it_is() {
    let (status, page) = send(get("/joblog_extension")).await;
    assert_eq!(status, StatusCode::OK);
    // Not written out in the tests.
    assert!(
        page.contains("The app couldn't put the extension in a folder for your browser."),
        "{page}"
    );
}

#[tokio::test]
async fn a_resume_written_as_fields_is_read_back_whole() {
    use recruiter::cv_match::cv_match_model;
    let db = seeded().await;
    let resume = cv_match_model::resume_fields(&db, 21).await.unwrap().unwrap();
    assert_eq!(resume.name, "Mette Sørensen");
    assert_eq!(resume.intro, ["Backend-udvikler."]);
    assert!(resume.links.is_empty());
    assert_eq!(resume.experience.len(), 1);
    assert_eq!(resume.experience[0].bullets, ["Byggede en API"]);
    assert_eq!(resume.skills, ["PHP"]);
    assert_eq!(resume.experience_heading, "Experience");
    assert_eq!(cv_match_model::resume_fields(&db, 22).await.unwrap(), None);
}
