//! A device pairs once with the code the window shows, then reaches the app with its token, and
//! only at the computer's tailnet name; a removed device can't any more. A paired device reads
//! the saved matches and sends job posts to be matched with the profile.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use device_protocol::{
    EVALUATE_PATH, EVALUATION_DONE, EVALUATION_FAILED, EVALUATION_WORKING, EVALUATIONS_PATH, EvaluateAsk, Evaluation,
    Evaluations, HELLO_PATH, Hello, MATCH_PATH, MATCHES_PATH, MatchDetails, Matches, PAIR_PATH, PairAnswer, PairAsk,
    PairingLink,
};
use http_body_util::BodyExt;
use jobfinder::account::key_vault::KeyVault;
use jobfinder::context::Context;
use jobfinder::db;
use jobfinder::devices::{device_server, device_service};
use jobfinder::taxonomy::Taxonomy;
use sqlx::Executor;
use tower::ServiceExt;

const TAILNET_NAME: &str = "pc.tail1234.ts.net";
const HOST: &str = "pc.tail1234.ts.net:8766";

static TAXONOMY: LazyLock<Arc<Taxonomy>> = LazyLock::new(|| Arc::new(Taxonomy::load()));

/// The app's context, with Tailscale's name just read.
async fn context() -> Arc<Context> {
    let db = db::open_in_memory().await.unwrap();
    let context = Context::new(db, TAXONOMY.clone(), KeyVault::in_memory());
    context.devices.set_tailnet_name(Some(TAILNET_NAME.to_string()), now());
    Arc::new(context)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

async fn send(context: &Arc<Context>, request: Request<Body>) -> (StatusCode, String) {
    let response = device_server::router(Arc::clone(context))
        .oneshot(request)
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

fn pair_request(host: &str, code: &str) -> Request<Body> {
    let ask = PairAsk {
        code: code.to_string(),
        name: "Pixel 8".to_string(),
    };
    Request::post(PAIR_PATH)
        .header(header::HOST, host)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&ask).unwrap()))
        .unwrap()
}

fn hello_request(token: Option<&str>) -> Request<Body> {
    let mut request = Request::post(HELLO_PATH).header(header::HOST, HOST);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request.body(Body::empty()).unwrap()
}

fn shown_code(context: &Context) -> String {
    let link = device_service::new_pairing_link(context, now()).unwrap();
    assert_eq!(link.host, TAILNET_NAME);
    PairingLink::parse(&link.to_link()).unwrap().code
}

#[tokio::test]
async fn a_device_pairs_once_and_then_says_hello_with_its_token() {
    let context = context().await;
    let code = shown_code(&context);
    let (status, body) = send(&context, pair_request(HOST, &code)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let token = serde_json::from_str::<PairAnswer>(&body).unwrap().token;

    let (status, body) = send(&context, hello_request(Some(&token))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let hello: Hello = serde_json::from_str(&body).unwrap();
    assert_eq!(hello.device_name, "Pixel 8");

    let devices = device_service::devices(&context).await.unwrap();
    assert_eq!(devices.len(), 1);
    assert!(devices[0].last_seen_at.is_some());

    // Only the token's hash is kept on the computer.
    let parsed = device_protocol::Token::parse(&token).unwrap();
    let kept = context.key_vault.device_token_hash(parsed.device_id).unwrap().unwrap();
    assert_ne!(kept, parsed.secret);
    assert_eq!(kept, device_service::token_hash(&parsed.secret));

    // The code worked once.
    let (status, _) = send(&context, pair_request(HOST, &code)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_its_own_token_a_device_is_refused() {
    let context = context().await;
    let code = shown_code(&context);
    let (_, body) = send(&context, pair_request(HOST, &code)).await;
    let token = serde_json::from_str::<PairAnswer>(&body).unwrap().token;
    let (device_id, _) = token.split_once('.').unwrap();
    let wrong = format!("{device_id}.{}", "0".repeat(64));
    for token in [None, Some("not a token"), Some(wrong.as_str())] {
        let (status, _) = send(&context, hello_request(token)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{token:?}");
    }
}

#[tokio::test]
async fn a_removed_device_is_refused() {
    let context = context().await;
    let code = shown_code(&context);
    let (_, body) = send(&context, pair_request(HOST, &code)).await;
    let token = serde_json::from_str::<PairAnswer>(&body).unwrap().token;
    let device = device_service::devices(&context).await.unwrap().remove(0);
    device_service::remove(&context, device.id).await.unwrap();
    let (status, _) = send(&context, hello_request(Some(&token))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_the_tailnet_name_is_answered() {
    let context = context().await;
    let code = shown_code(&context);
    for host in ["127.0.0.1:8766", "localhost:8766", "evil.example"] {
        let (status, _) = send(&context, pair_request(host, &code)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host}");
    }
    // The refusals didn't use the code up.
    let (status, _) = send(&context, pair_request(HOST, &code)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn without_tailscale_no_pairing_code_is_shown() {
    let db = db::open_in_memory().await.unwrap();
    let context = Context::new(db, TAXONOMY.clone(), KeyVault::in_memory());
    context.devices.set_tailnet_name(None, now());
    assert!(device_service::new_pairing_link(&context, now()).is_err());
}

/// One saved match with one requirement met.
const SAVED_MATCH: &str = "
INSERT INTO matches (id, job_title, company, job_url, job_text, score, points, max_points, tag, created_at)
VALUES (21, 'PHP-udvikler', 'Firma', 'https://www.jobindex.dk/job/1', 'Vi søger en PHP-udvikler.', 0.86, 9, 10, 'top', 1760000000);
INSERT INTO match_items (match_id, criterion, text, verdict, decided_by, reason, sort_order)
VALUES (21, 'requirements', 'PHP', 'met', 'exact', 'Named in the CV.', 0);
";

/// A profile to match job posts with.
const PROFILE: &str = "
INSERT INTO profile (id, name, title, about, created_at, updated_at)
VALUES (1, 'Sofie Andersen', 'Udvikler', 'PHP, Laravel, MySQL', 1760000000, 1760000000);
";

/// How many times a test asks for a device's evaluations before giving up on one finishing.
const EVALUATION_POLLS: usize = 200;

/// How long a test waits between asking, in milliseconds.
const EVALUATION_POLL_MS: u64 = 25;

/// A paired device's token.
async fn paired(context: &Arc<Context>) -> String {
    let code = shown_code(context);
    let (_, body) = send(context, pair_request(HOST, &code)).await;
    serde_json::from_str::<PairAnswer>(&body).unwrap().token
}

fn post(path: &str, token: Option<&str>, body: serde_json::Value) -> Request<Body> {
    let mut request = Request::post(path)
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request.body(Body::from(body.to_string())).unwrap()
}

/// The evaluation id once it is no longer working.
async fn finished(context: &Arc<Context>, token: &str, id: u64) -> Evaluation {
    for _ in 0..EVALUATION_POLLS {
        let (status, body) = send(context, post(EVALUATIONS_PATH, Some(token), serde_json::json!({}))).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let evaluations: Evaluations = serde_json::from_str(&body).unwrap();
        let evaluation = evaluations.evaluations.into_iter().find(|e| e.id == id).unwrap();
        if evaluation.state != EVALUATION_WORKING {
            return evaluation;
        }
        tokio::time::sleep(std::time::Duration::from_millis(EVALUATION_POLL_MS)).await;
    }
    panic!("evaluation {id} never finished");
}

#[tokio::test]
async fn only_a_paired_device_reads_matches_or_sends_posts() {
    let context = context().await;
    for path in [MATCHES_PATH, MATCH_PATH, EVALUATE_PATH, EVALUATIONS_PATH] {
        let body = serde_json::json!({"id": 21, "text": "PHP"});
        let (status, _) = send(&context, post(path, None, body)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn a_device_reads_the_lists_and_a_saved_match() {
    let context = context().await;
    context.db().execute(sqlx::raw_sql(SAVED_MATCH)).await.unwrap();
    let token = paired(&context).await;

    let (status, body) = send(&context, post(MATCHES_PATH, Some(&token), serde_json::json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let shortlist: Matches = serde_json::from_str(&body).unwrap();
    assert_eq!(shortlist.list, device_protocol::SHORTLIST);
    assert!(shortlist.lists.iter().any(|list| list.name == "all"));

    let (status, body) = send(
        &context,
        post(MATCHES_PATH, Some(&token), serde_json::json!({"list": "all"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let all: Matches = serde_json::from_str(&body).unwrap();
    assert_eq!(all.list, "all");
    assert_eq!(all.matches.len(), 1);
    assert_eq!(all.matches[0].id, 21);
    assert_eq!(all.matches[0].about, "86% · Top match");

    let (status, body) = send(&context, post(MATCH_PATH, Some(&token), serde_json::json!({"id": 21}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let details: MatchDetails = serde_json::from_str(&body).unwrap();
    assert_eq!(details.job_url, "https://www.jobindex.dk/job/1");
    let rows: Vec<_> = details.sections.iter().flat_map(|section| &section.rows).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].verdict, "met");

    let (status, _) = send(&context, post(MATCH_PATH, Some(&token), serde_json::json!({"id": 99}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_post_sent_without_a_profile_says_so() {
    let context = context().await;
    let token = paired(&context).await;
    let ask = serde_json::to_value(EvaluateAsk {
        text: "PHP-udvikler\nVi søger en PHP-udvikler.".to_string(),
        ..EvaluateAsk::default()
    })
    .unwrap();
    let (status, body) = send(&context, post(EVALUATE_PATH, Some(&token), ask)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let started: Evaluation = serde_json::from_str(&body).unwrap();
    assert_eq!(started.sent, "PHP-udvikler");
    let evaluation = finished(&context, &token, started.id).await;
    assert_eq!(evaluation.state, EVALUATION_FAILED);
    assert!(evaluation.said.contains("no profile"), "{}", evaluation.said);
}

#[tokio::test]
async fn without_a_model_a_post_sent_is_the_free_match_and_not_saved() {
    let context = context().await;
    context.db().execute(sqlx::raw_sql(PROFILE)).await.unwrap();
    let token = paired(&context).await;
    let ask = serde_json::json!({"text": "PHP-udvikler\nDin profil\n- Erfaring med PHP\n- Kendskab til Docker"});
    let (_, body) = send(&context, post(EVALUATE_PATH, Some(&token), ask)).await;
    let started: Evaluation = serde_json::from_str(&body).unwrap();
    let evaluation = finished(&context, &token, started.id).await;
    assert_eq!(evaluation.state, EVALUATION_DONE, "{}", evaluation.said);
    assert_eq!(evaluation.match_id, None);
    assert!(evaluation.percent.is_some());
    assert!(evaluation.said.starts_with("Not saved"), "{}", evaluation.said);
}

#[tokio::test]
async fn an_empty_post_is_refused() {
    let context = context().await;
    let token = paired(&context).await;
    let (status, body) = send(
        &context,
        post(EVALUATE_PATH, Some(&token), serde_json::json!({"text": "  "})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn a_page_the_device_showed_is_read_there_not_fetched() {
    let context = context().await;
    context.db().execute(sqlx::raw_sql(PROFILE)).await.unwrap();
    let token = paired(&context).await;
    let description = "Vi søger en erfaren PHP-udvikler til vores team i Odense. ".repeat(5);
    let html = format!(
        r#"<html><body><h1 data-testid="jobsearch-JobInfoHeader-title">PHP-udvikler</h1>
          <div data-testid="inlineHeader-companyName">Acme A/S</div>
          <div id="jobDescriptionText"><p>{description}</p></div></body></html>"#
    );
    let ask = serde_json::to_value(EvaluateAsk {
        url: "https://dk.indeed.com/viewjob?jk=0123456789abcdef&__cf_chl_rt_tk=abc".to_string(),
        html,
        ..EvaluateAsk::default()
    })
    .unwrap();
    let (status, body) = send(&context, post(EVALUATE_PATH, Some(&token), ask)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let started: Evaluation = serde_json::from_str(&body).unwrap();
    // The link as Indeed writes it, without the browser's challenge token.
    assert_eq!(started.sent, "https://dk.indeed.com/viewjob?jk=0123456789abcdef");
    let evaluation = finished(&context, &token, started.id).await;
    assert_eq!(evaluation.state, EVALUATION_DONE, "{}", evaluation.said);
}

#[tokio::test]
async fn a_page_needs_a_web_link() {
    let context = context().await;
    let token = paired(&context).await;
    let ask = serde_json::json!({"url": "file:///etc/passwd", "html": "<html></html>"});
    let (status, body) = send(&context, post(EVALUATE_PATH, Some(&token), ask)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}
