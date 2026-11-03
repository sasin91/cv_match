//! A device pairs once with the code the window shows, then reaches the app with its token, and
//! only at the computer's tailnet name; a removed device can't any more.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use device_protocol::{HELLO_PATH, Hello, PAIR_PATH, PairAnswer, PairAsk, PairingLink};
use http_body_util::BodyExt;
use jobfinder::account::key_vault::KeyVault;
use jobfinder::context::Context;
use jobfinder::db;
use jobfinder::devices::{device_server, device_service};
use jobfinder::taxonomy::Taxonomy;
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
