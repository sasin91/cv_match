//! An error the match page shows: JSON {error} with its status, plus need_key when the user has no
//! key to use for it and retryable when the language model may answer if asked again. Failures that
//! aren't the user's input are written to the log as well, so they can be found after the page is
//! closed.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

use crate::llm::LlmException;

#[derive(Debug)]
pub struct PageError {
    pub status: StatusCode,
    pub message: String,
    /// More fields of the JSON answer: need_key, retryable.
    pub extra: Map<String, Value>,
}

impl PageError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        PageError {
            status,
            message: message.into(),
            extra: Map::new(),
        }
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "That saved match was not found.")
    }

    /// A failure on the app's side, logged under where (the endpoint) with the message shown.
    pub fn failed(where_: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        eprintln!("match ({where_}): {message}");
        Self::new(StatusCode::BAD_GATEWAY, message)
    }

    /// There is no key to use for this: the page links to the API key page (402).
    pub fn need_key(message: impl Into<String>) -> Self {
        let mut error = Self::new(StatusCode::PAYMENT_REQUIRED, message);
        error.extra.insert("need_key".to_string(), Value::Bool(true));
        error
    }

    /// A failed model call, logged with the provider's answer (502).
    pub fn llm(where_: &str, e: &LlmException) -> Self {
        eprintln!("match ({where_}): {}", e.details());
        let mut error = Self::new(StatusCode::BAD_GATEWAY, e.message.clone());
        error.extra.insert("retryable".to_string(), Value::Bool(e.retryable));
        error
    }
}

impl IntoResponse for PageError {
    fn into_response(self) -> Response {
        let mut body = self.extra;
        body.insert("error".to_string(), json!(self.message));
        (self.status, Json(Value::Object(body))).into_response()
    }
}

impl From<sqlx::Error> for PageError {
    fn from(e: sqlx::Error) -> Self {
        eprintln!("match: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The database couldn't answer: {e}"),
        )
    }
}

impl From<askama::Error> for PageError {
    fn from(e: askama::Error) -> Self {
        eprintln!("match: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The page couldn't be printed: {e}"),
        )
    }
}
