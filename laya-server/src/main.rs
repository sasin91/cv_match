//! laya-server: Laya's POST /v1/systemone on ONNX Runtime, a drop-in for `python -m laya.serve`
//! serving the English checkpoint. Configuration is from the environment, as in Python:
//!
//!   LAYA_CHECKPOINT  folder with rl_agent_config.json and tokenizer/ (required)
//!   LAYA_ONNX        the exported laya.onnx, its .data file beside it (required)
//!   LAYA_HOST        bind address (127.0.0.1)
//!   LAYA_PORT        bind port (8080)
//!   LAYA_API_KEY     if set, require `Authorization: Bearer <it>`
//!   LAYA_THREADS     cap on ONNX Runtime's intra-op threads

use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Map, Value, json};

use laya_server::agent::{Laya, PredictError};
use laya_server::question::QuestionType;
use laya_server::sequence::serialize_state;
use laya_server::text::repr_str;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;

/// The only checkpoint this server has, and the name Recruiter asks for.
const MODEL: &str = "english";

/// laya-serve's request limits (serve.py MAX_*), refused before anything is tokenized.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_QUESTIONS: usize = 64;
const MAX_STATE_CHARS: usize = 50_000;
const MAX_CHOICE_OPTIONS: usize = 100;
const MAX_SCORE_LEVELS: usize = 32;
const MAX_TOTAL_OPTIONS: usize = 512;

/// Request fields laya-serve acts on that this server does not implement; refused rather than
/// ignored, so a caller never gets an answer computed differently from what it asked for.
const UNSUPPORTED_FIELDS: [&str; 7] = [
    "task",
    "lang",
    "lang_guess",
    "min_confidence",
    "max_len",
    "head_max_len",
    "hooks",
];

struct App {
    laya: Laya,
    api_key: Option<String>,
    checkpoint: String,
}

/// A refused request: the status and FastAPI's `detail` message.
type Refusal = (StatusCode, String);

/// An error response in FastAPI's shape: `{"detail": "..."}`.
fn fail(status: StatusCode, detail: impl Into<String>) -> Response {
    (status, axum::Json(json!({ "detail": detail.into() }))).into_response()
}

#[tokio::main]
async fn main() {
    if let Err(message) = serve().await {
        eprintln!("laya-server: {message}");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), String> {
    let checkpoint = required_env("LAYA_CHECKPOINT")?;
    let onnx = required_env("LAYA_ONNX")?;
    let threads = match env::var("LAYA_THREADS") {
        Ok(v) => Some(
            v.trim()
                .parse::<usize>()
                .map_err(|_| format!("LAYA_THREADS must be a number, got {v:?}"))?,
        ),
        Err(_) => None,
    };
    let host = env::var("LAYA_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let port = match env::var("LAYA_PORT") {
        Ok(v) => v
            .trim()
            .parse::<u16>()
            .map_err(|_| format!("LAYA_PORT must be a port number, got {v:?}"))?,
        Err(_) => DEFAULT_PORT,
    };
    let api_key = env::var("LAYA_API_KEY").ok().filter(|k| !k.is_empty());

    let started = Instant::now();
    let laya = Laya::load(&PathBuf::from(&checkpoint), &PathBuf::from(&onnx), threads)?;
    eprintln!(
        "laya-server: loaded {onnx} in {:.1}s",
        started.elapsed().as_secs_f64()
    );

    let app = Arc::new(App {
        laya,
        api_key,
        checkpoint,
    });
    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/systemone", post(systemone))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .map_err(|e| format!("cannot listen on {host}:{port}: {e}"))?;
    eprintln!("laya-server: listening on http://{host}:{port}");
    axum::serve(listener, router)
        .await
        .map_err(|e| e.to_string())
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("{name} is not set"))
}

/// Whether the request carries the bearer key, when one is configured.
fn authorized(app: &App, headers: &HeaderMap) -> bool {
    let Some(key) = &app.api_key else { return true };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    constant_time_eq(given.as_bytes(), key.as_bytes())
}

/// Compare without leaking, through timing, how much of the key a guess got right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Liveness for anyone; what is loaded only for an authorized caller, as laya-serve does.
async fn health(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if !authorized(&app, &headers) {
        return axum::Json(json!({ "status": "ok" })).into_response();
    }
    axum::Json(json!({
        "status": "ok",
        "loaded": [MODEL],
        "backend": "onnx",
        "checkpoint": app.checkpoint,
    }))
    .into_response()
}

async fn systemone(State(app): State<Arc<App>>, headers: HeaderMap, body: Bytes) -> Response {
    if !authorized(&app, &headers) {
        return fail(StatusCode::UNAUTHORIZED, "invalid or missing bearer token");
    }
    let body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return fail(
                StatusCode::BAD_REQUEST,
                format!("request body is not valid JSON: {e}"),
            );
        }
    };
    let request = match check_request(&body) {
        Ok(r) => r,
        Err((status, detail)) => return fail(status, detail),
    };

    let started = Instant::now();
    let worker = Arc::clone(&app);
    let result = tokio::task::spawn_blocking(move || {
        worker.laya.predict(&request.state, &request.questions)
    })
    .await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut result = match result {
        Ok(Ok(result)) => result,
        Ok(Err(PredictError::Invalid(detail))) => {
            return fail(StatusCode::UNPROCESSABLE_ENTITY, detail);
        }
        Ok(Err(PredictError::Failed(detail))) => {
            eprintln!("laya-server: inference failed: {detail}");
            return fail(StatusCode::INTERNAL_SERVER_ERROR, "inference failed");
        }
        Err(e) => {
            eprintln!("laya-server: inference task failed: {e}");
            return fail(StatusCode::INTERNAL_SERVER_ERROR, "inference failed");
        }
    };
    result["routing"] = routing(&app, request.model_given);

    let mut response = axum::Json(result).into_response();
    let timing = format!("{elapsed_ms:.2}");
    if let Ok(v) = HeaderValue::from_str(&format!("inference;dur={timing}")) {
        response.headers_mut().insert("server-timing", v);
    }
    if let Ok(v) = HeaderValue::from_str(&timing) {
        response.headers_mut().insert("x-inference-time-ms", v);
    }
    response
}

struct Request {
    state: Value,
    questions: Map<String, Value>,
    model_given: bool,
}

/// laya-serve's checks before inference: shape, size limits, refused fields, the model name.
fn check_request(body: &Value) -> Result<Request, Refusal> {
    let Some(body) = body.as_object().filter(|b| b.contains_key("questions")) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "request body must be an object with a 'questions' field".to_string(),
        ));
    };
    let state = body.get("state").cloned().unwrap_or(Value::Null);
    if state.is_null() {
        return Err((StatusCode::BAD_REQUEST, "'state' is required".to_string()));
    }
    let Some(questions) = body["questions"].as_object() else {
        return Err((
            StatusCode::BAD_REQUEST,
            "'questions' must be an object".to_string(),
        ));
    };
    check_limits(&state, questions)?;
    for field in UNSUPPORTED_FIELDS {
        if body.get(field).is_some_and(|v| !v.is_null()) {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                format!(
                    "'{field}' is not supported by laya-server; drop it or use the Python laya-serve"
                ),
            ));
        }
    }
    let model_given = match body.get("model") {
        None | Some(Value::Null) => false,
        Some(Value::String(m)) if m == MODEL => true,
        Some(other) => {
            let shown = other
                .as_str()
                .map(repr_str)
                .unwrap_or_else(|| other.to_string());
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("unknown model {shown}; laya-server serves only '{MODEL}'"),
            ));
        }
    };
    Ok(Request {
        state,
        questions: questions.clone(),
        model_given,
    })
}

/// The size limits laya-serve enforces (serve.py `_check_request_limits`).
fn check_limits(state: &Value, questions: &Map<String, Value>) -> Result<(), Refusal> {
    let too_large = |detail: String| Err((StatusCode::PAYLOAD_TOO_LARGE, detail));
    if questions.len() > MAX_QUESTIONS {
        return too_large(format!(
            "too many questions ({} > {MAX_QUESTIONS})",
            questions.len()
        ));
    }
    let mut total_options = 0;
    for (id, question) in questions {
        let criteria = question.get("criteria");
        let kind = question.get("type").and_then(Value::as_str);
        let count = match criteria {
            Some(Value::Array(a)) => a.len(),
            Some(Value::Object(o)) => o.len(),
            _ => continue,
        };
        if kind == Some(QuestionType::Choice.name()) {
            total_options += count;
            if count > MAX_CHOICE_OPTIONS {
                return too_large(format!(
                    "too many choice options for {} ({count} > {MAX_CHOICE_OPTIONS})",
                    repr_str(id)
                ));
            }
        } else if kind == Some(QuestionType::Score.name()) && criteria.is_some_and(Value::is_array)
        {
            total_options += count;
            if count > MAX_SCORE_LEVELS {
                return too_large(format!(
                    "too many score levels for {} ({count} > {MAX_SCORE_LEVELS})",
                    repr_str(id)
                ));
            }
        }
    }
    if total_options > MAX_TOTAL_OPTIONS {
        return too_large(format!(
            "too many answer options across questions ({total_options} > {MAX_TOTAL_OPTIONS})"
        ));
    }
    let state_chars = serialize_state(state).chars().count();
    if state_chars > MAX_STATE_CHARS {
        return too_large(format!(
            "state too large ({state_chars} > {MAX_STATE_CHARS} chars)"
        ));
    }
    Ok(())
}

/// The routing record laya-serve adds; there is one checkpoint, so it is always chosen.
fn routing(app: &App, model_given: bool) -> Value {
    let reason = if model_given {
        format!("explicit model={}", repr_str(MODEL))
    } else {
        "the only checkpoint served".to_string()
    };
    json!({ "model": MODEL, "repo": app.checkpoint, "reason": reason, "detection": null, "workflow": null })
}
