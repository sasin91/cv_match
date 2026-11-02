//! An error the server answers the Jobnet extension with: JSON {error} with its status, plus
//! need_key when the user has no key to use for it and retryable when the language model may
//! answer if asked again. The work's own errors (service_error) come in with the status their
//! kind answers with; failures that aren't the user's input were written to the log when they
//! were made, so they can be found after the browser tab is closed.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

use crate::service_error::{ErrorKind, ServiceError};

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    /// More fields of the JSON answer: need_key, retryable.
    pub extra: Map<String, Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
            extra: Map::new(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = self.extra;
        body.insert("error".to_string(), json!(self.message));
        (self.status, Json(Value::Object(body))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        crate::log_line!("extension_api: {e}");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("The database couldn't answer: {e}"),
        )
    }
}

impl From<ServiceError> for ApiError {
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
