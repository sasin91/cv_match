//! A failed model request, with a message fit to show
//! the user. `retryable` is true for rate limits and overload, where trying again later can
//! work. `response` is the provider's HTTP answer when there was one.

use std::fmt;

use super::llm_response::LlmResponse;

#[derive(Debug, Clone, PartialEq)]
pub struct LlmException {
    pub message: String,
    pub retryable: bool,
    pub response: Option<LlmResponse>,
}

impl LlmException {
    /// A failure before any answer, that trying again won't fix.
    pub fn new(message: impl Into<String>) -> Self {
        LlmException {
            message: message.into(),
            retryable: false,
            response: None,
        }
    }

    pub fn retryable(message: impl Into<String>) -> Self {
        LlmException {
            retryable: true,
            ..Self::new(message)
        }
    }

    /// A failure the provider's answer explains.
    pub fn answered(message: impl Into<String>, retryable: bool, response: LlmResponse) -> Self {
        LlmException {
            message: message.into(),
            retryable,
            response: Some(response),
        }
    }

    /// The message plus the provider's response, for the log.
    pub fn details(&self) -> String {
        match &self.response {
            None => self.message.clone(),
            Some(response) => format!("{} ({})", self.message, response.summary()),
        }
    }
}

impl fmt::Display for LlmException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LlmException {}
