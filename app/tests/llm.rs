//! The adapters send what the providers' APIs take and read what they answer, against a stand-in
//! for each API on 127.0.0.1.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::IntoResponse;
use jobfinder::llm::{Anthropic, Effort, Llm, Openai};
use serde_json::{Value, json};

/// A request the stand-in got: its path, headers and JSON body.
#[derive(Debug, Clone)]
struct Received {
    path: String,
    headers: HeaderMap,
    body: Value,
}

/// What the stand-in answers every request with, and what it got.
struct StandIn {
    status: StatusCode,
    headers: Vec<(&'static str, &'static str)>,
    body: String,
    received: Mutex<Vec<Received>>,
}

async fn answer(State(stand_in): State<Arc<StandIn>>, uri: Uri, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    stand_in.received.lock().unwrap().push(Received {
        path: uri.path().to_string(),
        headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    });
    let mut response = (stand_in.status, stand_in.body.clone()).into_response();
    for (name, value) in &stand_in.headers {
        response.headers_mut().insert(*name, value.parse().unwrap());
    }
    response
}

/// A stand-in answering status, headers and body, and its address.
async fn stand_in(status: u16, headers: Vec<(&'static str, &'static str)>, body: Value) -> (Arc<StandIn>, String) {
    let stand_in = Arc::new(StandIn {
        status: StatusCode::from_u16(status).unwrap(),
        headers,
        body: body.to_string(),
        received: Mutex::new(Vec::new()),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(answer).with_state(stand_in.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (stand_in, address)
}

fn only_request(stand_in: &StandIn) -> Received {
    let received = stand_in.received.lock().unwrap();
    assert_eq!(received.len(), 1, "{received:?}");
    received[0].clone()
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": { "title": { "type": "string" } },
        "required": ["title"],
        "additionalProperties": false,
    })
}

fn chat_answer(content: &str) -> Value {
    json!({ "choices": [{ "finish_reason": "stop", "message": { "content": content, "refusal": null } }] })
}

const OPENAI_KEY: &str = "sk-proj-0123456789abcdef";
const ANTHROPIC_KEY: &str = "sk-ant-api03-0123456789abcdef";

#[tokio::test]
async fn openai_gets_a_strict_schema_and_answers_json() {
    let (stand_in, address) = stand_in(200, vec![], chat_answer(r#"{"title":"PHP-udvikler"}"#)).await;
    let openai = Openai::new(OPENAI_KEY, "gpt-5.4-mini", &format!("{address}/v1")).unwrap();
    let answer = openai
        .structured("Read the post.", "Vi søger", &schema(), Effort::Low)
        .await
        .unwrap();
    assert_eq!(answer, json!({ "title": "PHP-udvikler" }));

    let request = only_request(&stand_in);
    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(request.headers["authorization"], format!("Bearer {OPENAI_KEY}"));
    assert_eq!(
        request.body,
        json!({
            "model": "gpt-5.4-mini",
            "messages": [
                { "role": "system", "content": "Read the post." },
                { "role": "user", "content": "Vi søger" },
            ],
            "reasoning_effort": "low",
            "store": false,
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": "answer", "strict": true, "schema": schema() },
            },
        })
    );
}

#[tokio::test]
async fn openai_says_why_it_failed() {
    let (_, address) = stand_in(
        200,
        vec![],
        json!({ "choices": [{ "message": { "content": null, "refusal": "I can't help with that." } }] }),
    )
    .await;
    let openai = Openai::new(OPENAI_KEY, "gpt-5.4-mini", &address).unwrap();
    let failed = openai.structured("s", "u", &schema(), Effort::Low).await.unwrap_err();
    assert_eq!(failed.message, "OpenAI declined: I can't help with that.");
    assert!(!failed.retryable);

    let (_, address) = stand_in(
        401,
        vec![],
        json!({ "error": { "message": format!("Incorrect API key provided: {OPENAI_KEY}."), "code": "invalid_api_key" } }),
    )
    .await;
    let openai = Openai::new(OPENAI_KEY, "gpt-5.4-mini", &address).unwrap();
    let failed = openai.structured("s", "u", &schema(), Effort::Low).await.unwrap_err();
    assert_eq!(
        failed.message,
        "OpenAI rejected the API key: Incorrect API key provided: sk-…"
    );
    assert!(!failed.details().contains("0123456789"), "the key stays out of the log");

    let (_, address) = stand_in(
        429,
        vec![("retry-after", "20")],
        json!({ "error": { "message": "Rate limit reached for requests.", "code": "rate_limit_exceeded" } }),
    )
    .await;
    let openai = Openai::new(OPENAI_KEY, "gpt-5.4-mini", &address).unwrap();
    let failed = openai.structured("s", "u", &schema(), Effort::Low).await.unwrap_err();
    assert_eq!(
        failed.message,
        "OpenAI rate limited the request: Rate limit reached for requests. Try again in 20 seconds."
    );
    assert!(failed.retryable);
    assert!(failed.details().contains("retry-after: 20"), "{}", failed.details());

    let (_, address) = stand_in(
        429,
        vec![],
        json!({ "error": { "message": "You exceeded your current quota.", "code": "insufficient_quota" } }),
    )
    .await;
    let openai = Openai::new(OPENAI_KEY, "gpt-5.4-mini", &address).unwrap();
    let failed = openai.structured("s", "u", &schema(), Effort::Low).await.unwrap_err();
    assert_eq!(
        failed.message,
        "OpenAI says the key has no quota left: You exceeded your current quota."
    );
    assert!(!failed.retryable);

    let (_, address) = stand_in(200, vec![], chat_answer("Sure! Here is the title.")).await;
    let openai = Openai::new(OPENAI_KEY, "gpt-5.4-mini", &address).unwrap();
    let failed = openai.structured("s", "u", &schema(), Effort::Low).await.unwrap_err();
    assert_eq!(
        failed.message,
        "OpenAI answered with something other than the JSON asked for."
    );
    assert!(failed.retryable);
}

#[tokio::test]
async fn a_local_model_without_a_key_gets_no_authorization() {
    let (stand_in, address) = stand_in(200, vec![], chat_answer(r#"{"title":"x"}"#)).await;
    let llm = Llm::use_key("local", "", "llama3.1:8b", &format!("{address}/v1/")).unwrap();
    llm.structured("s", "u", &schema(), Effort::Medium).await.unwrap();
    let request = only_request(&stand_in);
    assert_eq!(request.path, "/v1/chat/completions");
    assert!(!request.headers.contains_key("authorization"));
    assert_eq!(request.body["model"], "llama3.1:8b");
}

#[tokio::test]
async fn a_local_model_that_isnt_running_is_named() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/v1", listener.local_addr().unwrap());
    drop(listener);
    let llm = Llm::use_key("local", "", "llama3.1:8b", &address).unwrap();
    let failed = llm.structured("s", "u", &schema(), Effort::Low).await.unwrap_err();
    assert!(
        failed
            .message
            .starts_with(&format!("The local model at {address} could not be reached: ")),
        "{}",
        failed.message
    );
    assert!(failed.message.ends_with(". Is it running?"));
    assert!(failed.retryable);
}

#[tokio::test]
async fn openai_searches_the_web_through_the_responses_api() {
    let (stand_in, address) = stand_in(
        200,
        vec![],
        json!({
            "status": "completed",
            "output": [
                { "type": "web_search_call" },
                { "type": "message", "content": [{ "type": "output_text", "text": r#"{"title":"Found"}"# }] },
            ],
            "usage": { "input_tokens": 1200, "output_tokens": 80 },
        }),
    )
    .await;
    let llm = Llm {
        adapter: jobfinder::llm::Adapter::Openai(Openai::new(OPENAI_KEY, "gpt-5.4-mini", &address).unwrap()),
        server_in_use: None,
    };
    let found = llm
        .web_search("Find jobs.", "PHP", &schema(), &["jobnet.dk"], "DK", Effort::Low)
        .await
        .unwrap();
    assert_eq!(found.answer, json!({ "title": "Found" }));
    assert_eq!((found.input_tokens, found.output_tokens), (1200, 80));
    let request = only_request(&stand_in);
    assert_eq!(request.path, "/responses");
    assert_eq!(
        request.body["tools"],
        json!([{
            "type": "web_search",
            "filters": { "allowed_domains": ["jobnet.dk"] },
            "user_location": { "type": "approximate", "country": "DK" },
        }])
    );
    assert_eq!(request.body["text"]["format"]["strict"], true);

    let anthropic = Llm::use_key("anthropic", ANTHROPIC_KEY, "", "").unwrap();
    let failed = anthropic
        .web_search("s", "u", &schema(), &[], "", Effort::Low)
        .await
        .unwrap_err();
    assert_eq!(
        failed.message,
        "Searching the web needs an OpenAI key; this key is for another provider."
    );
}

#[tokio::test]
async fn claude_gets_the_schema_effort_and_fallbacks() {
    let (stand_in, address) = stand_in(
        200,
        vec![],
        json!({
            "stop_reason": "end_turn",
            "content": [{ "type": "thinking", "thinking": "" }, { "type": "text", "text": r#"{"title":"PHP-udvikler"}"# }],
        }),
    )
    .await;
    let anthropic = Anthropic::new(ANTHROPIC_KEY, "claude-sonnet-5-5", &address).unwrap();
    let answer = anthropic
        .structured("Read the post.", "Vi søger", &schema(), Effort::High)
        .await
        .unwrap();
    assert_eq!(answer, json!({ "title": "PHP-udvikler" }));

    let request = only_request(&stand_in);
    assert_eq!(request.path, "/v1/messages");
    assert_eq!(request.headers["x-api-key"], ANTHROPIC_KEY);
    assert_eq!(request.headers["anthropic-version"], "2026-10-21");
    assert_eq!(request.headers["anthropic-beta"], "server-side-fallback-2026-07-01");
    assert!(!request.headers.contains_key("authorization"));
    assert_eq!(
        request.body,
        json!({
            "model": "claude-sonnet-5-5",
            "max_tokens": 16000,
            "system": "Read the post.",
            "messages": [{ "role": "user", "content": "Vi søger" }],
            "output_config": { "effort": "high", "format": { "type": "json_schema", "schema": schema() } },
            "fallbacks": "default",
        })
    );
}

#[tokio::test]
async fn claude_says_why_it_failed() {
    let (_, address) = stand_in(
        200,
        vec![],
        json!({ "stop_reason": "refusal", "stop_details": { "type": "refusal", "explanation": "Not this." }, "content": [] }),
    )
    .await;
    let anthropic = Anthropic::new(ANTHROPIC_KEY, "claude-sonnet-5-5", &address).unwrap();
    let failed = anthropic
        .structured("s", "u", &schema(), Effort::Low)
        .await
        .unwrap_err();
    assert_eq!(failed.message, "Claude declined: Not this.");

    let (_, address) = stand_in(
        429,
        vec![("anthropic-ratelimit-requests-remaining", "0")],
        json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "Number of requests has exceeded your rate limit." } }),
    )
    .await;
    let anthropic = Anthropic::new(ANTHROPIC_KEY, "claude-sonnet-5-5", &address).unwrap();
    let failed = anthropic
        .structured("s", "u", &schema(), Effort::Low)
        .await
        .unwrap_err();
    assert_eq!(
        failed.message,
        "Claude rate limited the request: Number of requests has exceeded your rate limit."
    );
    assert!(failed.retryable);
    assert!(failed.details().contains("anthropic-ratelimit-requests-remaining: 0"));

    let (_, address) = stand_in(
        529,
        vec![],
        json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } }),
    )
    .await;
    let anthropic = Anthropic::new(ANTHROPIC_KEY, "claude-sonnet-5-5", &address).unwrap();
    let failed = anthropic
        .structured("s", "u", &schema(), Effort::Low)
        .await
        .unwrap_err();
    assert_eq!(failed.message, "Claude failed: Overloaded");
    assert!(failed.retryable);
}
