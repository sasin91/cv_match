//! Why the app's work (matches::match_service, account::account_service and the others) couldn't
//! be done, in words to show the user, with what kind of failure it is. The web server answers it with a status (matches::page_error); a
//! desktop window shows the message. Failures that aren't the user's input are written to the
//! log when they are made, so they can be found after the page or window is closed.

use crate::llm::LlmException;

/// What went wrong, as far as the one asking needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// What was sent can't be used (missing text, an unknown kind).
    Invalid,
    /// What was sent isn't shaped as asked (an unknown joblog status).
    Malformed,
    /// What was sent is too long.
    TooLarge,
    /// The saved match or version isn't there.
    NotFound,
    /// Too many of these in a short time.
    TooMany,
    /// There's no API key to use for this; the API key page fixes it.
    NeedKey,
    /// A call to another service or a step of the work failed.
    Failed,
    /// The language model's call failed; retryable says whether asking again may work.
    Llm { retryable: bool },
    /// The app itself failed: the database, the key store, making a file.
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    pub kind: ErrorKind,
    pub message: String,
}

impl ServiceError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        ServiceError {
            kind,
            message: message.into(),
        }
    }

    pub fn not_found() -> Self {
        Self::new(ErrorKind::NotFound, "That saved match was not found.")
    }

    /// A failure on the app's side, logged under where (the work's name, like "match/pdf") with
    /// the message shown.
    pub fn failed(where_: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        crate::log_line!("{where_}: {message}");
        Self::new(ErrorKind::Failed, message)
    }

    /// The app's own failure, logged under where with the message shown.
    pub fn internal(where_: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        crate::log_line!("{where_}: {message}");
        Self::new(ErrorKind::Internal, message)
    }

    /// There is no key to use for this.
    pub fn need_key(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NeedKey, message)
    }

    /// A failed model call, logged with the provider's answer.
    pub fn llm(where_: &str, e: &LlmException) -> Self {
        crate::log_line!("{where_}: {}", e.details());
        Self::new(ErrorKind::Llm { retryable: e.retryable }, e.message.clone())
    }
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ServiceError {}

impl From<sqlx::Error> for ServiceError {
    fn from(e: sqlx::Error) -> Self {
        crate::log_line!("database: {e}");
        Self::new(ErrorKind::Internal, format!("The database couldn't answer: {e}"))
    }
}
