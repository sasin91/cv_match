//! The local server answers the match page's requests as the page expects, and only from its
//! own page.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use recruiter::account::key_vault::KeyVault;
use recruiter::context::Context;
use recruiter::db;
use recruiter::laya::laya_client::{self, LayaClient};
use recruiter::server::{self, AppState};
use recruiter::taxonomy::Taxonomy;
use serde_json::{Value, json};
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
INSERT INTO matches (id, trongate_user_id, job_title, company, job_url, job_text, score, points, max_points, tag, created_at)
VALUES (21, 3, 'PHP-udvikler', 'Firma <A/S>', 'https://www.jobindex.dk/job/1', 'Vi søger en PHP-udvikler.\nSend din ansøgning til job@firma.dk. Skriv \"Backend\" i emnefeltet.', 0.86, 9, 10, 'top', 1760000000),
       (22, 3, 'Mekaniker', '', NULL, 'Mekaniker søges', 0.2, 1, 10, 'poor', 1760000100);
INSERT INTO documents (id, kind, name, text, written_at) VALUES (1, 'cv', 'cv', 'PHP', 1760000000);
INSERT INTO match_documents (match_id, document_id) VALUES (21, 1), (22, 1);
INSERT INTO match_items (match_id, criterion, text, verdict, decided_by, reason, sort_order)
VALUES (21, 'requirements', 'PHP', 'met', 'exact', 'Named in the CV.', 0),
       (21, 'location', 'Bor i København', 'missing', 'llm', 'Lives in Slagelse.', 1);
INSERT INTO match_rejections (match_id, reason, note, created_at) VALUES (22, 'commute', '1h each way', 0);
UPDATE matches SET rejected_at = 1 WHERE id = 22;
INSERT INTO documents (id, kind, text, written_at) VALUES (5, 'resume', 'Mette Sørensen', 1760000200);
INSERT INTO match_documents (match_id, document_id) VALUES (21, 5);
INSERT INTO match_resumes (document_id, name, intro) VALUES (5, 'Mette Sørensen', 'Backend-udvikler.');
INSERT INTO match_resume_entries (id, document_id, section, title, sort_order) VALUES (1, 5, 'experience', 'Udvikler', 0);
INSERT INTO match_resume_lines (document_id, entry_id, kind, text, sort_order)
VALUES (5, 1, 'bullet', 'Byggede en API', 0), (5, NULL, 'skill', 'PHP', 1);
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
    server::router(Arc::new(AppState::new(
        Arc::new(Context::new(db, TAXONOMY.clone(), user_id, KeyVault::in_memory())),
        PORT,
    )))
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
    let (status, page) = send(get("/match")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains(r#"<li class="history-top" id="saved-match-21"><a href="match/21" data-id="21">"#),
        "{page}"
    );
    assert!(page.contains("PHP-udvikler · Firma &#60;A/S&#62;") || page.contains("PHP-udvikler · Firma &lt;A/S&gt;"));
    assert!(page.contains("likely disqualifier: Bor i København"));
    assert!(
        !page.contains("saved-match-22"),
        "rejected matches stay out of the shortlist"
    );
    let (_, rejected) = send(get("/match?list=rejected")).await;
    assert!(rejected.contains("rejected: Too long commute (1h each way)"));
    assert!(rejected.contains(r#"<a href="match?list=rejected" aria-current="page">Rejected</a>"#));
}

#[tokio::test]
async fn a_saved_match_prints_with_its_notices_and_post() {
    let (status, result) = send(get("/match/result/21")).await;
    assert_eq!(status, StatusCode::OK);
    for part in [
        r#"data-saved-id="21""#,
        r#"data-percent="86""#,
        "Likely disqualifier: where you live",
        "Subject line: <code>Backend</code>",
        r#"href="mailto:job@firma.dk?subject=Backend""#,
        r#"action="match/submit_reject/21""#,
        r#"class="document" data-kind="application" data-id="21""#,
        r#"<template class="match-job" data-url="https://www.jobindex.dk/job/1">"#,
    ] {
        assert!(result.contains(part), "{part} in {result}");
    }
    let (status, row) = send(get("/match/result/21?view=row")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(row.contains(r#"<li class="batch-row" data-score="86" data-saved-id="21">"#));
    assert!(row.contains("www.jobindex.dk"));
    let (status, missing) = send(get("/match/result/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing, r#"{"error":"That saved match was not found."}"#);
}

#[tokio::test]
async fn the_free_match_reads_both_texts_and_saves_nothing() {
    let (status, result) = send(post_json(
        "/match/quick_match",
        r#"{"job_text": "PHP-udvikler\nDin profil\n- Erfaring med PHP\n- Kendskab til Docker", "cv_text": "Udvikler\nPHP, Laravel, MySQL"}"#,
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(result.contains(r#"data-saved-id="""#));
    assert!(result.contains(r#"<template class="cv-found""#));
    assert!(!result.contains("submit_reject"));
    let (status, empty) = send(post_json("/match/quick_match", r#"{"job_text": "", "cv_text": "x"}"#)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(empty, r#"{"error":"Paste or upload both the CV and the job post."}"#);
}

/// An app whose Laya folder has a laya.onnx that isn't a model.
async fn app_with_broken_laya() -> (axum::Router, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("recruiter-laya-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(laya_client::ONNX_FILE), "not a model").unwrap();
    let db = seeded().await;
    let user_id = server::local_user(&db).await.unwrap();
    let mut context = Context::new(db, TAXONOMY.clone(), user_id, KeyVault::in_memory());
    context.laya = LayaClient::from_folder(&folder);
    assert!(context.laya.is_some());
    (server::router(Arc::new(AppState::new(Arc::new(context), PORT))), folder)
}

#[tokio::test]
async fn a_laya_model_that_cant_load_is_said_under_the_score() {
    let (app, folder) = app_with_broken_laya().await;
    let (status, result) = send_to(
        &app,
        post_json(
            "/match/quick_match",
            r#"{"job_text": "PHP-udvikler\nDin profil\n- Erfaring med PHP", "cv_text": "Udvikler\nPHP, Laravel"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(
        result.contains(
            r#"<p id="laya-answer" class="muted">No Laya reading this time. Laya couldn&#39;t load its model from"#
        ),
        "{result}"
    );
    assert!(result.contains("The score above doesn&#39;t depend on it."));

    let job = json!({"job_title": "PHP-udvikler", "requirements": [{"text": "PHP", "kind": "skill"}]});
    let input = json!({ "job": job, "profile": {"skill": ["PHP"]}, "job_text": "", "cv_text": "" });
    let (status, scored) = send_to(&app, post_json("/match/match", &input.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{scored}");
    let scored: Value = serde_json::from_str(&scored).unwrap();
    assert_eq!(scored["laya"], Value::Null);
    assert!(
        scored["laya_error"]
            .as_str()
            .unwrap()
            .starts_with("Laya couldn't load its model from"),
        "{scored}"
    );
    std::fs::remove_dir_all(folder).unwrap();
}

#[tokio::test]
async fn without_a_laya_model_a_match_has_no_laya_line() {
    let job = json!({"job_title": "PHP-udvikler", "requirements": [{"text": "PHP", "kind": "skill"}]});
    let input = json!({ "job": job, "profile": {"skill": ["PHP"]}, "job_text": "", "cv_text": "" });
    let (status, scored) = send(post_json("/match/match", &input.to_string())).await;
    assert_eq!(status, StatusCode::OK, "{scored}");
    let scored: Value = serde_json::from_str(&scored).unwrap();
    assert_eq!(scored["laya"], Value::Null);
    assert!(scored.get("laya_error").is_none(), "{scored}");
}

#[tokio::test]
async fn a_link_to_this_computer_or_the_home_network_is_not_fetched() {
    for url in [
        "http://127.0.0.1:8765/match",
        "http://192.168.1.1/",
        "http://localhost/",
    ] {
        let (status, answer) = send(post_json("/match/fetch_job", &json!({ "url": url }).to_string())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url}");
        assert!(answer.contains("error"), "{answer}");
    }
    let (status, answer) = send(get("/match/nonsense")).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(answer, r#"{"error":"match/nonsense isn't in the desktop app."}"#);
}

#[tokio::test]
async fn other_sites_cant_post_here() {
    let request = Request::post("/match/submit_reject/21")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("reasons%5B%5D=salary"))
        .unwrap();
    let (status, _) = send(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = send(
        Request::get("/match")
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
        "/match_module/css/match.css",
        "/match_module/js/app.js",
        "/js/trongate-mx.min.js",
    ] {
        let response = app().await.oneshot(get(path)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

/// POST match/pdf_text with these bytes: the status and the JSON answer.
async fn pdf_text(app: &axum::Router, bytes: Vec<u8>) -> (StatusCode, serde_json::Value) {
    let request = Request::post("/match/pdf_text")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/pdf")
        .body(Body::from(bytes))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn an_uploaded_pdf_is_read_by_the_app_not_the_browser() {
    let app = app().await;
    // The app's own PDF of a saved job post reads back as its text.
    let (status, _, _, job_pdf) = pdf(&app, r#"{"id": 22, "kind": "job"}"#).await;
    assert_eq!(status, StatusCode::OK);
    let (status, answer) = pdf_text(&app, job_pdf).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer["text"].as_str().unwrap().contains("Mekaniker"), "{answer}");

    // A file that isn't a PDF says why, so the text can be pasted instead.
    let (status, answer) = pdf_text(&app, b"not a pdf".to_vec()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(answer["error"].as_str().unwrap().contains("Paste its text"), "{answer}");
}

/// A saved match's PDF: its status, Content-Type and Content-Disposition, and its bytes.
async fn pdf(app: &axum::Router, json: &str) -> (StatusCode, String, String, Vec<u8>) {
    let response = app.clone().oneshot(post_json("/match/pdf", json)).await.unwrap();
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
    let (status, panel) = send(get("/match/joblog_panel/21")).await;
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

    let (status, entry) = send(get("/match/joblog/21")).await;
    assert_eq!(status, StatusCode::OK);
    let entry: serde_json::Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(entry["sections"][0]["fields"][0]["value"], "PHP-udvikler");
    assert_eq!(entry["sections"][1]["fields"][3]["value"], true);
    assert_eq!(entry["logged_at"], serde_json::Value::Null);

    let (status, answer) = send(get("/match/joblog/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(answer, r#"{"error":"That saved match was not found."}"#);
}

#[tokio::test]
async fn the_extension_marks_a_match_as_on_jobnet() {
    let app = app().await;
    let (_, history) = send_to(&app, get("/match/history")).await;
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
            "/match/joblog_status",
            r#"{"id": "21", "status": "Soegt", "logged": false}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{logged}");
    assert!(logged.contains(r#"<section class="joblog-section joblog-saved" data-applied-on=""#));
    assert!(!logged.contains("<time"));
    let (_, history) = send_to(&app, get("/match/history")).await;
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    assert_eq!(history["not_joblogged"][0]["id"], 21);
    assert_eq!(history["not_joblogged"][0]["jobnet_status"], "Soegt");

    // The extension saved it on Jobnet.
    let request = Request::post("/match/joblog_status")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, EXTENSION)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"id": 21, "status": "Samtale", "logged": true}"#))
        .unwrap();
    let (status, logged) = send_to(&app, request).await;
    assert_eq!(status, StatusCode::OK, "{logged}");
    assert!(logged.contains(" checked>"));
    assert!(logged.contains("Entered on <time datetime=\""));
    let (_, history) = send_to(&app, get("/match/history")).await;
    let history: serde_json::Value = serde_json::from_str(&history).unwrap();
    assert_eq!(history["not_joblogged"], serde_json::json!([]));
    let (_, entry) = send_to(&app, get("/match/joblog/21")).await;
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
        post_json("/match/joblog_status", r#"{"id": 21, "status": "Done"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        answer,
        r#"{"error":"Pick one of Jobnet's statuses: Ikke søgt, Søgt, Samtale."}"#
    );
    let (status, _) = send_to(
        &app,
        post_json("/match/joblog_status", r#"{"id": 22000, "status": "Soegt"}"#),
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
    use recruiter::matches::match_model;
    let db = seeded().await;
    let resume = match_model::resume_fields(&db, 21).await.unwrap().unwrap();
    assert_eq!(resume.name, "Mette Sørensen");
    assert_eq!(resume.intro, ["Backend-udvikler."]);
    assert!(resume.links.is_empty());
    assert_eq!(resume.experience.len(), 1);
    assert_eq!(resume.experience[0].bullets, ["Byggede en API"]);
    assert_eq!(resume.skills, ["PHP"]);
    assert_eq!(resume.experience_heading, "Experience");
    assert_eq!(match_model::resume_fields(&db, 22).await.unwrap(), None);
}

fn post_form(path: &str, form: &str) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form.to_string()))
        .unwrap()
}

/// The page a form's redirect leads to, after checking it redirects there.
async fn followed(app: &axum::Router, request: Request<Body>) -> String {
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/account");
    send_to(app, get("/account")).await.1
}

#[tokio::test]
async fn the_api_key_page_keeps_a_key_per_provider_and_picks_one_per_use() {
    let app = app().await;
    let (status, page) = send_to(&app, get("/account")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains(r#"<option value="openai" selected>OpenAI</option>"#),
        "{page}"
    );
    assert!(page.contains(r#"placeholder="Default: gpt-5.4-mini or claude-sonnet-5-5""#));
    assert!(!page.contains("saved-key"));

    let page = followed(
        &app,
        post_form(
            "/account/submit_key",
            "provider=anthropic&model=&api_key=sk-ant-api03-0123456789-abcd&base_url=",
        ),
    )
    .await;
    assert!(page.contains("Your Anthropic (Claude) key is saved."), "{page}");
    assert!(page.contains("key ending in <code>…abcd</code>"), "{page}");
    assert!(!page.contains("0123456789"), "the key is never shown again");
    assert!(
        page.contains(r#"<option value="openai" selected>OpenAI</option>"#),
        "the form offers the next provider: {page}"
    );

    let page = followed(
        &app,
        post_form(
            "/account/submit_key",
            "provider=openai&model=&api_key=sk-proj-0123456789abcdWXYZ&base_url=",
        ),
    )
    .await;
    assert!(
        page.contains("key ending in <code>…abcd</code>"),
        "the first key stays: {page}"
    );
    assert!(page.contains("key ending in <code>…WXYZ</code>"), "{page}");
    assert!(
        page.contains(r#"<option value="anthropic" selected>Anthropic (Claude) (…abcd)</option>"#),
        "the uses stay on the first key: {page}"
    );

    let page = followed(
        &app,
        post_form(
            "/account/submit_uses",
            "use_for_resume=openai&use_for_application=anthropic&use_for_job_evaluation=",
        ),
    )
    .await;
    assert!(page.contains("Saved which key each use runs on."), "{page}");
    assert!(page.contains(r#"<option value="openai" selected>OpenAI (…WXYZ)</option>"#));
    assert!(page.contains(r#"<option value="anthropic" selected>Anthropic (Claude) (…abcd)</option>"#));
    assert!(page.contains(r#"<option value="" selected>No key (switched off)</option>"#));

    let page = followed(&app, post_form("/account/submit_delete", "provider=anthropic")).await;
    assert!(page.contains("Your Anthropic (Claude) key is deleted."), "{page}");
    assert!(!page.contains("…abcd"));
    assert!(page.contains("key ending in <code>…WXYZ</code>"));

    let page = followed(&app, post_form("/account/submit_delete", "provider=openai")).await;
    assert!(!page.contains("saved-key"));
}

#[tokio::test]
async fn the_api_key_page_shows_what_is_wrong_with_the_form() {
    let app = app().await;
    let (status, page) = send_to(
        &app,
        post_form(
            "/account/submit_key",
            "provider=local&model=&api_key=&base_url=localhost%3A11434",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        page.contains("<li>A local model needs its model name, like llama3.1:8b.</li>"),
        "{page}"
    );
    assert!(page.contains(r#"<option value="local" selected>"#));
    assert!(page.contains(r#"value="localhost:11434""#), "what was typed comes back");

    let page = followed(
        &app,
        post_form(
            "/account/submit_key",
            "provider=local&model=llama3.1%3A8b&api_key=&base_url=http%3A%2F%2Flocalhost%3A11434%2Fv1",
        ),
    )
    .await;
    assert!(page.contains("<strong>A local model (Ollama, LM Studio)</strong>, model <code>llama3.1:8b</code>, at <code>http://localhost:11434/v1</code>."), "{page}");
}

#[tokio::test]
async fn checking_without_a_key_says_so() {
    let app = app().await;
    let page = followed(&app, post_form("/account/submit_check", "provider=openai")).await;
    assert!(page.contains("no key to check: save one first."), "{page}");
}

// ---------- the AI match and writing, on a stand-in for a local model ----------

/// A stand-in for an OpenAI-compatible model on 127.0.0.1: it answers each kind of request by
/// the schema it asks for, and counts the requests.
async fn stand_in_model() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use axum::routing::post;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        post(move |axum::Json(body): axum::Json<Value>| {
            let counted = counted.clone();
            async move {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let required = &body["response_format"]["json_schema"]["schema"]["required"];
                let asks = |key: &str| required.as_array().unwrap().iter().any(|k| k == key);
                let user = body["messages"][1]["content"].as_str().unwrap_or("").to_string();
                let answer = if asks("entries") {
                    profile_answer()
                } else if asks("requirements") {
                    job_answer()
                } else if asks("experience_years") {
                    cv_answer()
                } else if asks("verdicts") {
                    judged(&user)
                } else if asks("application") {
                    json!({ "post_language": "Danish", "employer_needs": "x", "standout": [], "application": "Kære Firma\n\nJeg søger stillingen." })
                } else {
                    resume_answer()
                };
                axum::Json(json!({
                    "choices": [{ "finish_reason": "stop", "message": { "content": answer.to_string(), "refusal": null } }]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (address, calls)
}

fn job_answer() -> Value {
    let requirement = |value: &str, kind: &str| json!({ "value": value, "english": value, "kind": kind, "required": true, "alt_group": "", "min_years": 0, "min_level": "" });
    json!({
        "text_en": "We seek a PHP developer who lives near Copenhagen.",
        "job_title": "PHP-udvikler",
        "job_title_en": "PHP developer",
        "company": "Firma A/S",
        "requirements": [requirement("PHP", "skill"), requirement("Erfaring med betalingsløsninger", "experience"), requirement("Bosat nær København", "location")],
        "responsibilities": ["Udvikle API'er"],
    })
}

fn profile_answer() -> Value {
    json!({
        "name": "Mette Sørensen", "email": "mette@example.com", "phone": "", "title": "Udvikler", "location": "Slagelse",
        "about": ["Backend-udvikler."],
        "entries": [{ "section": "experience", "title": "Udvikler", "organisation": "Firma", "location": "", "url": "",
                      "started": "2017-01", "ended": "", "summary": "", "bullets": ["Byggede betalingsløsninger"], "note": "" }],
        "skills": ["PHP"], "languages": [], "certificates": [],
    })
}

fn cv_answer() -> Value {
    json!({
        "name": "Mette Sørensen", "location": "Slagelse, 4200", "title": ["Udvikler"], "skill": ["PHP", "Laravel"],
        "language": ["Danish"], "certificate": [], "education": [], "experience_years": 8, "responsibilities": [],
        "text_en": "Mette Sørensen, developer. PHP, Laravel. Built payment flows.",
    })
}

/// Every requirement the user message lists ("- [id] (kind) text") as met.
fn judged(user: &str) -> Value {
    let verdicts: Vec<Value> = user
        .lines()
        .filter_map(|line| line.strip_prefix("- [")?.split(']').next())
        .map(
            |id| json!({ "id": id, "verdict": "met", "evidence": "Built payment flows", "reason": "Shown in the CV." }),
        )
        .collect();
    json!({ "verdicts": verdicts })
}

fn resume_answer() -> Value {
    json!({
        "post_language": "Danish", "employer_needs": "x", "standout": [],
        "keywords": [{ "term": "PHP", "basis": "8 years of PHP" }],
        "left_out": ["Go"],
        "resume": {
            "name": "Mette Sørensen", "title": "PHP-udvikler", "location": "Slagelse", "phone": "", "email": "", "links": [],
            "intro": ["Backend-udvikler med 8 års PHP."],
            "experience": [{ "title": "Udvikler", "organisation": "Firma", "location": "", "starts": "2017", "ends": "nu", "summary": "", "bullets": ["Byggede betalingsløsninger"], "note": "" }],
            "projects": [], "skills": ["PHP"], "languages": ["Dansk"], "education_note": "", "education": [],
            "experience_heading": "Erfaring", "skills_heading": "Kompetencer", "education_heading": "Uddannelse",
            "languages_heading": "Sprog", "projects_heading": "",
        },
    })
}

/// The seeded app with a local model saved as the user's key.
async fn app_with_model() -> (axum::Router, SqlitePool, Arc<std::sync::atomic::AtomicUsize>) {
    let db = seeded().await;
    let user_id = server::local_user(&db).await.unwrap();
    let vault = KeyVault::in_memory();
    let (base_url, calls) = stand_in_model().await;
    recruiter::account::account_model::save(&db, &vault, user_id, "local", "stand-in", &base_url, "")
        .await
        .unwrap();
    let app = server::router(Arc::new(AppState::new(
        Arc::new(Context::new(db.clone(), TAXONOMY.clone(), user_id, vault)),
        PORT,
    )));
    (app, db, calls)
}

fn post_html(path: &str, json: &Value) -> Request<Body> {
    let mut request = post_json(path, &json.to_string());
    request
        .headers_mut()
        .insert(header::ACCEPT, "text/html".parse().unwrap());
    request
}

/// The saved id a result prints.
fn saved_id(result: &str) -> i64 {
    let start = result.find(r#"data-saved-id=""#).unwrap() + r#"data-saved-id=""#.len();
    result[start..].split('"').next().unwrap().parse().unwrap()
}

#[tokio::test]
async fn the_ai_match_reads_judges_scores_and_saves() {
    let (app, db, _) = app_with_model().await;
    let (_, page) = send_to(&app, get("/match")).await;
    assert!(page.contains("aiReady: true"), "{page}");
    assert!(!page.contains("No AI model is set up."));

    let (status, job) = send_to(
        &app,
        post_json("/match/extract_job", r#"{"text": "PHP-udvikler søges"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    let job: Value = serde_json::from_str(&job).unwrap();
    assert_eq!(job["requirements"][1]["kind"], "experience");
    let (_, profile) = send_to(&app, post_json("/match/extract_cv", r#"{"text": "Mette Sørensen"}"#)).await;
    let profile: Value = serde_json::from_str(&profile).unwrap();
    assert_eq!(profile["location"], "Slagelse, 4200");

    let input = json!({ "job": job, "profile": profile, "job_text": "PHP-udvikler søges", "job_url": "", "cv_name": "", "cv_text": "Mette Sørensen, PHP" });
    let (status, result) = send_to(&app, post_html("/match/match", &input)).await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let id = saved_id(&result);
    assert!(result.contains("Built payment flows"), "the model's evidence: {result}");
    assert!(result.contains(r#"class="document" data-kind="application""#));
    assert!(result.contains("For Laya"));
    let found = recruiter::matches::match_model::find(&db, id, 3)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.cv_name, "Mette Sørensen", "the CV's own name");
    assert_eq!(found.company, "Firma A/S");
    let judged: Vec<&str> = found.items.iter().map(|item| item.decided_by.as_str()).collect();
    assert!(judged.contains(&"llm"), "{judged:?}");

    let (status, scored) = send_to(&app, post_json("/match/match", &input.to_string())).await;
    assert_eq!(status, StatusCode::OK);
    let scored: Value = serde_json::from_str(&scored).unwrap();
    assert!(
        scored["saved_id"].as_i64().unwrap() > id,
        "JSON for the résumé's comparison"
    );
    assert!(scored["result"]["index"].as_f64().unwrap() > 0.0);
}

#[tokio::test]
async fn writing_again_keeps_the_version_before_in_its_list() {
    let (app, _, _) = app_with_model().await;
    let (status, first) = send_to(
        &app,
        post_json("/match/write_application", r#"{"id": "21", "notes": "kort"}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert!(first.contains("Kære Firma"), "{first}");
    assert!(!first.contains("Earlier versions"));
    let (_, second) = send_to(
        &app,
        post_json("/match/write_application", r#"{"id": 21, "notes": ""}"#),
    )
    .await;
    assert!(second.contains("Earlier versions (1)"), "{second}");
    assert!(second.contains("by local/stand-in, with the notes “kort”"), "{second}");

    let (_, saved) = send_to(&app, get("/match/saved/21")).await;
    let saved: Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["application_text"], "Kære Firma\n\nJeg søger stillingen.");
    assert!(saved["application_written_at"].is_i64());
    let earlier = saved["earlier_versions"][0]["id"].as_i64().unwrap();

    let (status, content_type, ..) = pdf(
        &app,
        &json!({ "id": 21, "kind": "application", "document_id": earlier }).to_string(),
    )
    .await;
    assert_eq!(
        (status, content_type.as_str()),
        (StatusCode::OK, "application/pdf"),
        "an earlier version's PDF"
    );
    let (status, ..) = pdf(
        &app,
        &json!({ "id": 21, "kind": "resume", "document_id": earlier }).to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a version is only its own kind's");
}

#[tokio::test]
async fn a_tailored_resume_comes_back_with_its_keywords_and_compares() {
    let (app, db, _) = app_with_model().await;
    let (status, resume) = send_to(&app, post_json("/match/tailor_resume", r#"{"id": 21}"#)).await;
    assert_eq!(status, StatusCode::OK, "{resume}");
    assert!(
        resume.contains("Uses 1 of the post&#39;s keywords: PHP")
            || resume.contains("Uses 1 of the post&#x27;s keywords: PHP"),
        "{resume}"
    );
    assert!(resume.contains("Left out, since your CV doesn"));
    assert!(resume.contains(r#"data-structured="true""#));
    assert!(
        resume.contains("Earlier versions (1)"),
        "the seeded résumé stays: {resume}"
    );
    let fields = recruiter::matches::match_model::resume_fields(&db, 21)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fields.experience_heading, "Erfaring");
    assert_eq!(
        fields.projects_heading, "Projects",
        "an empty heading is the English one"
    );

    let (status, line) = send_to(&app, get("/match/resume_comparison/21/22")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        line.contains("The tailored résumé scores 20% poor against this post; your CV scored 86% top."),
        "{line}"
    );
    assert!(line.contains(r#"data-id="22""#));
}

#[tokio::test]
async fn a_key_switched_off_for_a_use_says_so() {
    let (app, db, calls) = app_with_model().await;
    recruiter::account::account_model::save_uses(
        &db,
        3,
        &recruiter::account::account_model::UseChoices {
            resume: Some("local".to_string()),
            application: None,
            job_evaluation: None,
        },
    )
    .await
    .unwrap();
    let (status, answer) = send_to(&app, post_json("/match/write_application", r#"{"id": 21}"#)).await;
    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    let answer: Value = serde_json::from_str(&answer).unwrap();
    assert_eq!(answer["need_key"], true);
    assert_eq!(
        answer["error"],
        "None of your API keys is picked for writing job applications. Pick one on the API key page."
    );
    // Evaluating job posts off: the free reader reads, and the match is still saved.
    let (status, job) = send_to(
        &app,
        post_json(
            "/match/extract_job",
            r#"{"text": "PHP-udvikler\nDin profil\n- Erfaring med PHP"}"#,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0, "no model call");
    let input = json!({ "job": serde_json::from_str::<Value>(&job).unwrap(), "profile": { "skill": ["PHP"] }, "job_text": "PHP-udvikler", "cv_text": "PHP" });
    let (_, scored) = send_to(&app, post_json("/match/match", &input.to_string())).await;
    let scored: Value = serde_json::from_str(&scored).unwrap();
    assert!(scored["saved_id"].is_i64(), "{scored}");
    let (_, result) = send_to(&app, get("/match/result/21")).await;
    assert!(
        result.contains("Your API key isn&#39;t on for writing the application.")
            || result.contains("Your API key isn&#x27;t on for writing the application."),
        "{result}"
    );
}

#[tokio::test]
async fn without_a_key_the_ai_endpoints_ask_for_one() {
    let (status, answer) = send(post_json("/match/write_application", r#"{"id": 21}"#)).await;
    assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
    assert!(answer.contains("Add your OpenAI or Anthropic API key, or a local model, on the API key page"));
}

#[tokio::test]
async fn the_apply_dialog_says_how_to_send_the_application() {
    let (status, dialog) = send(get("/match/application_delivery/21")).await;
    assert_eq!(status, StatusCode::OK, "{dialog}");
    assert!(dialog.contains("Apply by email"));
    assert!(dialog.contains(r#"value="job@firma.dk""#));
    assert!(dialog.contains(r#"href="mailto:job@firma.dk?subject=Backend""#));
    assert!(dialog.contains("Write the application first (Write application)."));
    let (status, _) = send(get("/match/application_delivery/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn edge_isnt_opened_without_an_extension_folder() {
    let (status, answer) = send(post_json("/joblog_extension/open_edge", "{}")).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.contains("There is no extension folder to load."), "{answer}");
}

/// A new database's app: nothing in it but the local user.
async fn new_app(vault: KeyVault) -> (axum::Router, SqlitePool) {
    let db = db::open_in_memory().await.unwrap();
    let user_id = server::local_user(&db).await.unwrap();
    let app = server::router(Arc::new(AppState::new(
        Arc::new(Context::new(db.clone(), TAXONOMY.clone(), user_id, vault)),
        PORT,
    )));
    (app, db)
}

/// Where a request redirects to.
async fn redirect_of(app: &axum::Router, request: Request<Body>) -> String {
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION].to_str().unwrap().to_string()
}

#[tokio::test]
async fn the_app_opens_on_the_dashboard_which_says_what_is_missing() {
    let (new, _) = new_app(KeyVault::in_memory()).await;
    assert_eq!(redirect_of(&new, get("/")).await, "/match");
    let (status, page) = send_to(&new, get("/match")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("No AI model is set up."), "{page}");
    assert!(page.contains("3 things need your attention."), "{page}");
    assert!(page.contains(r#"<a href="setup#ai">Set one up</a>"#), "{page}");
    assert!(page.contains(r#"<a href="setup#cv">Read your CV</a>"#), "{page}");
    assert!(page.contains("Nothing on the shortlist."), "{page}");
    let (_, rejected) = send_to(&new, get("/match?list=rejected")).await;
    assert!(rejected.contains("No AI model is set up."));
    // The list the page polls brings the findings along, so they keep up with it.
    let (_, latest) = send_to(&new, get("/match/history_list")).await;
    assert!(latest.contains(r#"id="dashboard-findings""#), "{latest}");
    assert!(latest.contains("Nothing on the shortlist."), "{latest}");

    // With a key in use, the alert goes and the AI row says which.
    let (base_url, _) = stand_in_model().await;
    let key = format!("provider=local&model=stand-in&api_key=&base_url={base_url}");
    send_to(&new, post_form("/account/submit_key", &key)).await;
    let (_, page) = send_to(&new, get("/match")).await;
    assert!(!page.contains("No AI model is set up."), "{page}");
    assert!(page.contains("A local model (Ollama, LM Studio), stand-in"), "{page}");
}

#[tokio::test]
async fn the_setup_suggests_an_api_key_without_room_for_a_model_and_waits_for_it_to_read_the_cv() {
    let (app, _) = new_app(KeyVault::in_memory()).await;
    let (status, page) = send_to(&app, get("/setup")).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("Suggested: an OpenAI API key."), "{page}");
    assert!(page.contains("<code>gpt-5.4-mini</code>"));
    assert!(page.contains("Set up an AI model first"));
    assert!(!page.contains(r#"action="setup/submit_cv""#));
    let (_, account) = send_to(&app, get("/account")).await;
    assert!(account.contains(r#"<a href="setup">Back to the setup</a>"#));
    assert!(account.contains(r#"<option value="openai" selected>OpenAI</option>"#));
}

#[tokio::test]
async fn the_setup_reads_the_cv_into_the_profile_the_match_page_uses() {
    let vault = KeyVault::in_memory();
    let (app, db) = new_app(vault).await;
    // The key saved on the first run goes back to the setup.
    let (base_url, _) = stand_in_model().await;
    let key = format!("provider=local&model=stand-in&api_key=&base_url={base_url}");
    assert_eq!(
        redirect_of(&app, post_form("/account/submit_key", &key)).await,
        "/setup"
    );
    let (_, page) = send_to(&app, get("/setup")).await;
    assert!(page.contains("A local model (Ollama, LM Studio), stand-in"), "{page}");
    assert!(page.contains(r#"action="setup/submit_cv""#));

    let (status, short) = send_to(&app, post_form("/setup/submit_cv", "cv_text=Mette&cv_name=cv.pdf")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(short.contains("must be at least 100 characters"), "{short}");
    assert!(
        short.contains(">Mette</textarea>"),
        "the text comes back as it was posted"
    );

    let cv = "Mette Sørensen, udvikler i Slagelse. ".repeat(4);
    let form = format!("cv_text={}&cv_name=cv.pdf", cv.replace(' ', "+"));
    assert_eq!(redirect_of(&app, post_form("/setup/submit_cv", &form)).await, "/setup");
    let (_, page) = send_to(&app, get("/setup")).await;
    assert!(page.contains("Your profile is filled in from the CV"), "{page}");
    assert!(
        page.contains("Your profile: Mette Sørensen, read from cv.pdf on "),
        "{page}"
    );
    assert!(page.contains(r#"name="replace""#));
    let profile = recruiter::profile::text_for_user(&db, 1).await.unwrap().unwrap();
    assert!(profile.contains("Udvikler · Firma"), "{profile}");
    // The dashboard says whose profile it is, and the match form matches with it.
    let (_, match_page) = send_to(&app, get("/match")).await;
    assert!(match_page.contains("Byggede betalingsløsninger"));
    assert!(
        match_page.contains("Mette Sørensen, read from cv.pdf on "),
        "{match_page}"
    );

    let (_, again) = send_to(&app, post_form("/setup/submit_cv", &form)).await;
    assert!(
        again.contains("Tick &#34;Replace my profile&#34;") || again.contains("Tick &quot;Replace my profile&quot;"),
        "{again}"
    );
}

#[tokio::test]
async fn the_setup_schedules_the_job_scan_and_stops_it() {
    let (app, _) = new_app(KeyVault::in_memory()).await;
    assert_eq!(
        redirect_of(&app, post_form("/setup/submit_schedule", "hours=8")).await,
        "/setup"
    );
    let (_, page) = send_to(&app, get("/setup")).await;
    assert!(page.contains("The job scan runs every 8 hours while you"), "{page}");
    assert!(page.contains("The job scan runs by itself: every 8 hours"));
    assert!(page.contains(r#"<option value="8" selected>Every 8 hours</option>"#));
    redirect_of(&app, post_form("/setup/submit_schedule", "hours=5")).await;
    let (_, refused) = send_to(&app, get("/setup")).await;
    assert!(
        refused.contains("5 hours isn&#39;t one of the intervals offered.") || refused.contains("5 hours isn&#x27;t"),
        "{refused}"
    );
    redirect_of(&app, post_form("/setup/submit_unschedule", "")).await;
    let (_, stopped) = send_to(&app, get("/setup")).await;
    assert!(stopped.contains("no longer runs by itself"));
    assert!(!stopped.contains("runs by itself: every"));
}

/// The page any form's redirect leads to.
async fn followed_to(app: &axum::Router, request: Request<Body>) -> String {
    let location = redirect_of(app, request).await;
    send_to(app, get(&location)).await.1
}

#[tokio::test]
async fn a_job_search_is_made_listed_and_scanned_only_on_an_openai_key() {
    let (app, db) = new_app(KeyVault::in_memory()).await;
    // Nothing to scan yet: the dashboard says so and offers no Scan now.
    let (_, page) = send_to(&app, get("/match")).await;
    assert!(page.contains("There is no job search to scan yet."), "{page}");
    assert!(!page.contains("job_scan/submit_scan"), "{page}");

    // The form says what's wrong, and keeps what was typed.
    let (status, form) = send_to(
        &app,
        post_form("/job_scan/submit/0", "name=Backend&tailor_from_score=60"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(form.contains("Say what to look for."), "{form}");
    assert!(form.contains(r#"value="Backend""#), "{form}");

    let saved =
        "name=Backend&keywords=PHP+udvikler&location=Odense&remote_ok=1&sites=jobnet.dk&tailor_from_score=60&active=1";
    let search = followed_to(&app, post_form("/job_scan/submit/0", saved)).await;
    assert!(search.contains("Your job search is saved."), "{search}");
    assert!(
        search.contains("PHP udvikler · Odense · remote too · on jobnet.dk · tailors from 60"),
        "{search}"
    );
    assert!(search.contains("No job posts found yet."), "{search}");
    let (_, list) = send_to(&app, get("/job_scan")).await;
    assert!(list.contains("Backend"), "{list}");
    assert!(list.contains("0 found, 0 matched · not scanned yet"), "{list}");

    // A local model can't search the web: Scan now says so, and nothing is scanned.
    let (base_url, _) = stand_in_model().await;
    let key = format!("provider=local&model=stand-in&api_key=&base_url={base_url}");
    send_to(&app, post_form("/account/submit_key", &key)).await;
    let back = followed_to(&app, post_form("/job_scan/submit_scan", "back=job_scan")).await;
    assert!(back.contains("which only OpenAI"), "{back}");
    let runs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM job_search_runs")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(runs, 0);
    let (_, page) = send_to(&app, get("/match")).await;
    assert!(page.contains("only OpenAI"), "{page}");
    assert!(!page.contains("job_scan/submit_scan"), "{page}");

    // Unknown searches are the not-found page; Scan now never goes back off the app.
    let (status, _) = send_to(&app, get("/job_scan/search/99")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = app
        .clone()
        .oneshot(post_form("/job_scan/submit_scan", "back=https://example.com"))
        .await
        .unwrap();
    assert_eq!(response.headers()[header::LOCATION], "/match");
}

#[tokio::test]
async fn dismissing_a_found_post_rejects_its_match_and_rejecting_the_match_dismisses_it() {
    let (app, db) = new_app(KeyVault::in_memory()).await;
    let saved = "name=Backend&keywords=PHP&tailor_from_score=60&active=1";
    followed_to(&app, post_form("/job_scan/submit/0", saved)).await;
    db.execute(sqlx::raw_sql(
        r#"
        INSERT INTO matches (id, trongate_user_id, job_title, company, job_text, score, points, max_points, tag, created_at)
        VALUES (5, 1, 'PHP developer', 'Acme', 'post', 0.8, 8, 10, 'good', 100);
        INSERT INTO scanned_job_posts (id, job_search_id, url, url_hash, title, status, match_id, found_at)
        VALUES (1, 1, 'https://jobnet.dk/1', 'x', 'PHP developer', 'matched', 5, 100);
        "#,
    ))
    .await
    .unwrap();
    let (_, search) = send_to(&app, get("/job_scan/search/1")).await;
    assert!(
        search.contains("80<span class=\"visually-hidden\"> of 100</span> · Good match"),
        "{search}"
    );
    assert!(search.contains("Open the match"), "{search}");

    assert_eq!(
        redirect_of(&app, post_form("/job_scan/submit_dismiss/1", "")).await,
        "/job_scan/search/1"
    );
    let rejected: Option<i64> = sqlx::query_scalar("SELECT rejected_at FROM matches WHERE id = 5")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(rejected.is_some());
    redirect_of(&app, post_form("/job_scan/submit_dismiss/1", "undo=1")).await;
    let status: String = sqlx::query_scalar("SELECT status FROM scanned_job_posts WHERE id = 1")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "matched");

    // Rejected on the match page, the post is dismissed with it.
    let reject = "reasons%5B%5D=salary";
    send_to(&app, post_form("/match/submit_reject/5", reject)).await;
    let status: String = sqlx::query_scalar("SELECT status FROM scanned_job_posts WHERE id = 1")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(status, "dismissed");
}
