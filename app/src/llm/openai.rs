//! llm/openai: OpenAI's Chat Completions API over
//! HTTPS, with a strict JSON schema response format and reasoning_effort. It also runs a local
//! OpenAI-compatible server (Ollama, LM Studio) at its base_url, where the key may be empty.
//! web_search() uses the Responses API with its web_search tool.

use serde_json::{Value, json};

use super::{Effort, LlmException, LlmResponse};

/// OpenAI's API.
pub const BASE_URL: &str = "https://api.openai.com/v1";

/// What messages call OpenAI's API.
const OPENAI_NAME: &str = "OpenAI";

/// What messages call a local server.
const LOCAL_NAME: &str = "The local model";

/// The name the response format gives the answer's schema.
const SCHEMA_NAME: &str = "answer";

#[derive(Debug, Clone)]
pub struct Openai {
    api_key: String,
    pub model: String,
    pub base_url: String,
    /// What messages call the provider: OpenAI, or the local model.
    name: &'static str,
    client: reqwest::Client,
}

/// web_search's answer and the tokens it took.
#[derive(Debug, Clone, PartialEq)]
pub struct WebSearchAnswer {
    pub answer: Value,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

impl Openai {
    /// OpenAI's API (or a stand-in for it at base_url, in tests).
    pub fn new(api_key: &str, model: &str, base_url: &str) -> Result<Self, LlmException> {
        Self::named(OPENAI_NAME, api_key, model, base_url)
    }

    /// A local OpenAI-compatible server at base_url.
    pub fn local(api_key: &str, model: &str, base_url: &str) -> Result<Self, LlmException> {
        Self::named(LOCAL_NAME, api_key, model, base_url)
    }

    fn named(name: &'static str, api_key: &str, model: &str, base_url: &str) -> Result<Self, LlmException> {
        Ok(Openai {
            api_key: api_key.to_string(),
            model: model.to_string(),
            base_url: base_url.to_string(),
            name,
            client: super::http_client(base_url)?,
        })
    }

    pub async fn structured(
        &self,
        system: &str,
        user: &str,
        schema: &Value,
        effort: Effort,
    ) -> Result<Value, LlmException> {
        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "reasoning_effort": effort.as_str(),
            // The user's CVs: OpenAI keeps nothing beyond its abuse checks.
            "store": false,
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": SCHEMA_NAME, "strict": true, "schema": schema },
            },
        });
        let answer = self.post("/chat/completions", &body).await?;

        let choice = answer.json()["choices"][0].clone();
        if let Some(refusal) = choice["message"]["refusal"].as_str() {
            return Err(self.declined(refusal, answer));
        }
        if choice["finish_reason"] == "length" {
            return Err(self.out_of_tokens(answer));
        }
        match choice["message"]["content"].as_str() {
            Some(content) if !content.is_empty() => super::decoded(self.name, content, &answer),
            _ => Err(self.no_answer(answer)),
        }
    }

    pub async fn web_search(
        &self,
        system: &str,
        user: &str,
        schema: &Value,
        domains: &[&str],
        country: &str,
        effort: Effort,
    ) -> Result<WebSearchAnswer, LlmException> {
        let mut tool = json!({ "type": "web_search" });
        if !domains.is_empty() {
            tool["filters"] = json!({ "allowed_domains": domains });
        }
        if !country.is_empty() {
            tool["user_location"] = json!({ "type": "approximate", "country": country });
        }
        let body = json!({
            "model": self.model,
            "instructions": system,
            "input": user,
            "tools": [tool],
            "reasoning": { "effort": effort.as_str() },
            "text": {
                "format": { "type": "json_schema", "name": SCHEMA_NAME, "strict": true, "schema": schema },
            },
            "store": false,
        });
        let answer = self.post("/responses", &body).await?;
        let response = answer.json();
        if response["status"] == "incomplete" {
            let why = response["incomplete_details"]["reason"].as_str().unwrap_or("unknown");
            let retryable = why != "max_output_tokens";
            let message = format!("{} stopped before finishing ({why}).", self.name);
            return Err(LlmException::answered(message, retryable, answer));
        }
        let text = match output_text(&response) {
            Ok(text) => text,
            Err(refusal) => return Err(self.declined(&refusal, answer)),
        };
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return Err(self.no_answer(answer));
        };
        Ok(WebSearchAnswer {
            answer: super::decoded(self.name, &text, &answer)?,
            input_tokens: response["usage"]["input_tokens"].as_i64().unwrap_or(0),
            output_tokens: response["usage"]["output_tokens"].as_i64().unwrap_or(0),
        })
    }

    /// POSTs body to the API at path and returns the provider's answer.
    async fn post(&self, path: &str, body: &Value) -> Result<LlmResponse, LlmException> {
        if self.api_key.is_empty() && self.name == OPENAI_NAME {
            return Err(LlmException::new(
                "OpenAI needs an API key. Add yours on the API key page.",
            ));
        }
        let authorization = format!("Bearer {}", self.api_key);
        // A local server without a key gets no Authorization header at all.
        let headers: &[(&str, &str)] = if self.api_key.is_empty() {
            &[]
        } else {
            &[("authorization", authorization.as_str())]
        };
        let url = format!("{}{path}", self.base_url.trim_end_matches('/'));
        let answer = super::post(&self.client, &url, headers, body)
            .await
            .map_err(|e| self.not_answered(&e))?;
        if answer.status < 400 && answer.is_json() {
            return Ok(answer);
        }
        Err(match answer.status {
            401 => super::rejected_key(self.name, answer),
            429 => self.rate_limited(answer),
            _ => super::failed(self.name, answer),
        })
    }

    /// The request got no answer; a local server says where it was looked for.
    fn not_answered(&self, error: &reqwest::Error) -> LlmException {
        if self.name == LOCAL_NAME && !error.is_timeout() {
            let name = format!("{LOCAL_NAME} at {}", self.base_url);
            let mut exception = super::not_answered(&name, error);
            exception.message.push_str(". Is it running?");
            return exception;
        }
        super::not_answered(self.name, error)
    }

    /// A 429 in the provider's own words: a used-up quota (billing) can't be waited out; a rate
    /// limit can, and says for how long when it can.
    fn rate_limited(&self, answer: LlmResponse) -> LlmException {
        let detail = answer.error_message();
        if answer.error_code() == "insufficient_quota" {
            let detail = if detail.is_empty() {
                "check the plan and billing.".to_string()
            } else {
                detail
            };
            let message = format!("{} says the key has no quota left: {detail}", self.name);
            return LlmException::answered(message, false, answer);
        }
        let mut message = if detail.is_empty() {
            format!("{} rate limited the request.", self.name)
        } else {
            format!("{} rate limited the request: {detail}", self.name)
        };
        let retry_after = answer.header("retry-after");
        if retry_after.parse::<f64>().is_ok() {
            message.push_str(&format!(" Try again in {retry_after} seconds."));
        } else if detail.is_empty() {
            message.push_str(" Try again in a minute.");
        }
        LlmException::answered(message, true, answer)
    }

    fn declined(&self, refusal: &str, answer: LlmResponse) -> LlmException {
        LlmException::answered(format!("{} declined: {refusal}", self.name), false, answer)
    }

    fn out_of_tokens(&self, answer: LlmResponse) -> LlmException {
        let message = format!("{} ran out of tokens before finishing.", self.name);
        LlmException::answered(message, false, answer)
    }

    fn no_answer(&self, answer: LlmResponse) -> LlmException {
        LlmException::answered(format!("{} returned no answer.", self.name), false, answer)
    }
}

/// The text of the Responses API's last output_text part (None without one), or Err with what a
/// refusal part said.
fn output_text(response: &Value) -> Result<Option<String>, String> {
    let mut text = None;
    let messages = response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "message");
    for item in messages {
        for part in item["content"].as_array().into_iter().flatten() {
            if part["type"] == "refusal" {
                return Err(part["refusal"].as_str().unwrap_or("").to_string());
            }
            if part["type"] == "output_text" {
                text = part["text"].as_str().map(str::to_string);
            }
        }
    }
    Ok(text)
}
