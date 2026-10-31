//! The provider's HTTP answer: status, headers and
//! body. Every LlmException made after an answer carries it, so the log (and the user) can see
//! why the request failed, such as which rate limit a 429 hit and when it resets, or what a
//! refused or cut-short answer said. The body is kept as the provider sent it, for the adapters
//! to read; what goes to the log or the page has any API key in it cut to its prefix.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// How much of the body a log line keeps, in characters.
const LOGGED_BODY_CHARACTERS: usize = 2000;

/// Headers worth a log line: rate limits, when to retry and the request id to quote to the
/// provider.
static LOGGED_HEADER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(retry-after|x-ratelimit-|anthropic-ratelimit-|x-request-id$|request-id$)").unwrap()
});

/// An API key as it may appear in an error body ("Incorrect API key provided: sk-ab...yz").
static API_KEY_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(sk-(?:ant-)?)[A-Za-z0-9_\-*.]{4,}").unwrap());

/// Runs of whitespace, which a log line keeps as one space.
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

#[derive(Clone, PartialEq)]
pub struct LlmResponse {
    pub status: u16,
    /// Lower-case name and value, in the order the provider sent them.
    pub headers: Vec<(String, String)>,
    /// The body as the provider sent it; redacted() before it is shown or logged.
    body: String,
}

impl fmt::Debug for LlmResponse {
    /// With the body redacted, like the log.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &redacted(&self.body))
            .finish()
    }
}

impl LlmResponse {
    pub fn new(status: u16, headers: Vec<(String, String)>, body: &str) -> Self {
        LlmResponse {
            status,
            headers,
            body: body.to_string(),
        }
    }

    /// A header's value, or "".
    pub fn header(&self, name: &str) -> &str {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    }

    /// The body as decoded JSON, or Null when it isn't.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }

    /// Whether the body is a JSON object or array, as a successful answer is.
    pub fn is_json(&self) -> bool {
        matches!(self.json(), Value::Object(_) | Value::Array(_))
    }

    /// The provider's own words on what went wrong (error.message), redacted, or "".
    pub fn error_message(&self) -> String {
        let message = self.error().get("message").map(text_of).unwrap_or_default();
        redacted(message.trim())
    }

    /// The provider's error code or type ("rate_limit_exceeded", "insufficient_quota"), or "".
    pub fn error_code(&self) -> String {
        let error = self.error();
        error
            .get("code")
            .filter(|code| !code.is_null())
            .or_else(|| error.get("type"))
            .map(text_of)
            .unwrap_or_default()
    }

    /// One line for the log: status, the headers that explain it, and the body.
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("HTTP {}", self.status)];
        for (name, value) in &self.headers {
            if LOGGED_HEADER_PATTERN.is_match(name) {
                parts.push(format!("{name}: {value}"));
            }
        }
        parts.push(format!("body: {}", self.logged_body()));
        parts.join("; ")
    }

    fn error(&self) -> serde_json::Map<String, Value> {
        match self.json().get("error") {
            Some(Value::Object(error)) => error.clone(),
            _ => serde_json::Map::new(),
        }
    }

    fn logged_body(&self) -> String {
        let body = redacted(&self.body);
        let body = WHITESPACE.replace_all(body.trim(), " ");
        if body.chars().count() > LOGGED_BODY_CHARACTERS {
            let cut: String = body.chars().take(LOGGED_BODY_CHARACTERS).collect();
            format!("{cut}…")
        } else {
            body.into_owned()
        }
    }
}

/// text with any API key in it cut to its prefix ("sk-…", "sk-ant-…").
fn redacted(text: &str) -> String {
    API_KEY_PATTERN.replace_all(text, "$1…").into_owned()
}

/// A JSON value as text: a string as itself, a number as its digits.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_in_the_body_is_cut_to_its_prefix() {
        let answer = LlmResponse::new(
            401,
            vec![],
            r#"{"error":{"message":"Incorrect API key provided: sk-proj-abcdef123456. Or sk-ant-api03-xyz9"}}"#,
        );
        assert_eq!(answer.error_message(), "Incorrect API key provided: sk-… Or sk-ant-…");
        assert!(!answer.summary().contains("abcdef"));
        assert!(!format!("{answer:?}").contains("abcdef"));
    }

    #[test]
    fn an_answer_is_read_as_the_provider_sent_it() {
        let answer = LlmResponse::new(200, vec![], r#"{"skill":"sk-learn"}"#);
        assert_eq!(answer.json()["skill"], "sk-learn");
    }

    #[test]
    fn the_summary_keeps_the_headers_that_explain_a_failure() {
        let answer = LlmResponse::new(
            429,
            vec![
                ("content-type".to_string(), "application/json".to_string()),
                ("retry-after".to_string(), "20".to_string()),
                ("x-ratelimit-remaining-requests".to_string(), "0".to_string()),
                ("x-request-id".to_string(), "req_1".to_string()),
            ],
            "{\"error\":\n  {\"code\": \"rate_limit_exceeded\", \"type\": \"requests\"}}",
        );
        assert_eq!(
            answer.summary(),
            r#"HTTP 429; retry-after: 20; x-ratelimit-remaining-requests: 0; x-request-id: req_1; body: {"error": {"code": "rate_limit_exceeded", "type": "requests"}}"#
        );
        assert_eq!(answer.error_code(), "rate_limit_exceeded");
        assert_eq!(answer.header("retry-after"), "20");
    }

    #[test]
    fn the_error_type_stands_in_for_a_missing_code() {
        let answer = LlmResponse::new(529, vec![], r#"{"error":{"type":"overloaded_error","code":null}}"#);
        assert_eq!(answer.error_code(), "overloaded_error");
        assert_eq!(answer.error_message(), "");
        assert!(!LlmResponse::new(200, vec![], "not json").is_json());
    }

    #[test]
    fn a_long_body_is_cut_in_the_log() {
        let answer = LlmResponse::new(500, vec![], &"é".repeat(LOGGED_BODY_CHARACTERS + 5));
        let summary = answer.summary();
        assert!(summary.ends_with("é…"));
        assert_eq!(summary.chars().filter(|c| *c == 'é').count(), LOGGED_BODY_CHARACTERS);
    }
}
