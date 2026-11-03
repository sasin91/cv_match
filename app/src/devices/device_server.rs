//! The app's server for paired devices: on 127.0.0.1:device_protocol::PORT only, reached from a
//! phone or a tablet through `tailscale serve`, which keeps the Host the device sent. It answers
//! only at the computer's tailnet name (so a web page on this computer can't use it), and, past
//! pairing, only a paired device's token (device_service::device_for).

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use device_protocol::{
    EVALUATE_PATH, EVALUATIONS_PATH, EvaluateAsk, Evaluation, Evaluations, HELLO_PATH, Hello, MATCH_PATH, MATCHES_PATH,
    MAX_EVALUATE_HTML, MatchAsk, MatchDetails, Matches, MatchesAsk, PAIR_PATH, PORT, PairAnswer, PairAsk,
};

use super::{device_matches, device_service, tailscale};
use crate::api_error::ApiError;
use crate::context::Context;

/// Bytes of JSON around a page a device sends (its link, the escaping of its HTML), at most: HTML
/// escaped as JSON can grow by its quotes and backslashes, so a page may take twice its size.
const EVALUATE_JSON_ROOM: usize = MAX_EVALUATE_HTML;

/// Most bytes evaluate reads of what a device sends: a page's HTML, and its JSON.
const EVALUATE_BYTES: usize = MAX_EVALUATE_HTML + EVALUATE_JSON_ROOM;

pub fn router(context: Arc<Context>) -> Router {
    Router::new()
        .route(PAIR_PATH, post(pair))
        .route(HELLO_PATH, post(hello))
        .route(MATCHES_PATH, post(matches))
        .route(MATCH_PATH, post(match_details))
        .route(EVALUATE_PATH, post(evaluate).layer(DefaultBodyLimit::disable()))
        .route(EVALUATIONS_PATH, post(evaluations))
        .fallback(not_here)
        .method_not_allowed_fallback(not_here)
        .layer(middleware::from_fn_with_state(
            Arc::clone(&context),
            only_at_the_tailnet_name,
        ))
        .with_state(context)
}

/// The device port on 127.0.0.1, taken at start; Err when it is taken.
pub async fn bind() -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, PORT))).await
}

/// Answers devices on listener (bind) until the process ends.
pub async fn serve_on(listener: tokio::net::TcpListener, context: Arc<Context>) -> std::io::Result<()> {
    axum::serve(listener, router(context)).await
}

/// POST v1/device/pair: a device trades the pairing code shown now for its token.
async fn pair(State(context): State<Arc<Context>>, Json(ask): Json<PairAsk>) -> Result<Json<PairAnswer>, ApiError> {
    Ok(Json(device_service::pair(&context, &ask, now()).await?))
}

/// POST v1/device/hello: what a paired device is paired with.
async fn hello(State(context): State<Arc<Context>>, headers: HeaderMap) -> Result<Json<Hello>, ApiError> {
    let device = device_service::device_for(&context, authorization(&headers), now()).await?;
    Ok(Json(device_service::hello(&device)))
}

/// POST v1/device/matches: a batch of a list of saved matches, the shortlist first.
async fn matches(
    State(context): State<Arc<Context>>,
    headers: HeaderMap,
    Json(ask): Json<MatchesAsk>,
) -> Result<Json<Matches>, ApiError> {
    device_service::device_for(&context, authorization(&headers), now()).await?;
    Ok(Json(device_matches::matches(&context, &ask).await?))
}

/// POST v1/device/match: one saved match.
async fn match_details(
    State(context): State<Arc<Context>>,
    headers: HeaderMap,
    Json(ask): Json<MatchAsk>,
) -> Result<Json<MatchDetails>, ApiError> {
    device_service::device_for(&context, authorization(&headers), now()).await?;
    Ok(Json(device_matches::match_details(&context, ask.id).await?))
}

/// POST v1/device/evaluate: a job post to match with the user's profile; answered as it starts.
/// The device's token is checked before its body (which may be a page's HTML) is read.
async fn evaluate(State(context): State<Arc<Context>>, request: Request) -> Result<Json<Evaluation>, ApiError> {
    let now = now();
    let device = device_service::device_for(&context, authorization(request.headers()), now).await?;
    let body = axum::body::to_bytes(request.into_body(), EVALUATE_BYTES)
        .await
        .map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "The page is too large to read."))?;
    let ask: EvaluateAsk = serde_json::from_slice(&body).map_err(|e| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("JobFinder couldn't read what was sent: {e}"),
        )
    })?;
    Ok(Json(device_matches::evaluate(&context, &device, &ask, now)?))
}

/// POST v1/device/evaluations: what became of the job posts this device sent.
async fn evaluations(State(context): State<Arc<Context>>, headers: HeaderMap) -> Result<Json<Evaluations>, ApiError> {
    let device = device_service::device_for(&context, authorization(&headers), now()).await?;
    Ok(Json(device_matches::evaluations(&context, &device)))
}

async fn not_here() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "JobFinder on this computer has no such page for devices.",
    )
}

/// Lets a request through only when its Host is the computer's tailnet name, as `tailscale
/// serve` passes it on. The name is read again when it is old and the Host isn't it (Tailscale
/// may have started since). What it runs is in the task manager's list.
async fn only_at_the_tailnet_name(State(context): State<Arc<Context>>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .unwrap_or("")
        .to_string();
    let now = now();
    let mut allowed = is_tailnet_host(&host, context.devices.tailnet_name().as_deref());
    if !allowed && context.devices.tailnet_name_is_old(now) {
        let state = tailscale::state(&context, PORT, now).await;
        allowed = is_tailnet_host(&host, state.name());
    }
    if !allowed {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "This is JobFinder's address for paired devices; it answers only at this computer's Tailscale name.",
        )
        .into_response();
    }
    let running = Arc::clone(&context.running);
    running.within(next.run(request)).await
}

/// Whether host (a Host header) is name, with or without the device port.
fn is_tailnet_host(host: &str, name: Option<&str>) -> bool {
    let Some(name) = name else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    host == name || host == format!("{name}:{PORT}")
}

/// The request's Authorization header.
fn authorization(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_tailnet_name_is_its_host() {
        let name = Some("pc.tail1234.ts.net");
        assert!(is_tailnet_host("pc.tail1234.ts.net:8766", name));
        assert!(is_tailnet_host("PC.tail1234.ts.net", name));
        assert!(!is_tailnet_host("127.0.0.1:8766", name));
        assert!(!is_tailnet_host("pc.tail1234.ts.net:8765", name));
        assert!(!is_tailnet_host("evil.example", name));
        assert!(!is_tailnet_host("pc.tail1234.ts.net:8766", None));
    }
}
