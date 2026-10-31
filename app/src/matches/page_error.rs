//! An error the match page shows: JSON {error} with its status, plus need_key when the user has no
//! key to use for it and retryable when the language model may answer if asked again. The work's
//! own errors (service_error) come in with the status their kind answers with; failures that
//! aren't the user's input were written to the log when they were made, so they can be found
//! after the page is closed.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

use crate::service_error::{ErrorKind, ServiceError};

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

impl From<ServiceError> for PageError {
    /// The work's error with the status its kind answers with; it was logged when it was made.
    fn from(e: ServiceError) -> Self {
        let status = match e.kind {
            ErrorKind::Invalid => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorKind::Malformed => StatusCode::BAD_REQUEST,
            ErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::TooMany => StatusCode::TOO_MANY_REQUESTS,
            ErrorKind::NeedKey => StatusCode::PAYMENT_REQUIRED,
            ErrorKind::Failed | ErrorKind::Llm { .. } => StatusCode::BAD_GATEWAY,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let mut error = Self::new(status, e.message);
        match e.kind {
            ErrorKind::NeedKey => {
                error.extra.insert("need_key".to_string(), Value::Bool(true));
            }
            ErrorKind::Llm { retryable } => {
                error.extra.insert("retryable".to_string(), Value::Bool(retryable));
            }
            _ => {}
        }
        error
    }
}
