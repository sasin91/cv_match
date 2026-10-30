//! account: the API key page, ported from modules/account/Account.php for the one local user.
//! The AI features of the CV match run on the user's own OpenAI or Anthropic API key, or on a
//! local OpenAI-compatible server (Ollama, LM Studio). The key is kept in Windows Credential
//! Manager and never shown again; only its last 4 characters are. Its uses (tailoring résumés,
//! writing applications, evaluating job posts) can each be switched off.
//!
//! Profile links, downloading your data and deleting the account stay in the PHP app: there is
//! no account on the desktop.

pub mod account_model;
pub mod key_vault;
pub mod saved_key;

use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use serde_json::json;

use self::account_model::{PROVIDERS, Saved};
use self::saved_key::SavedKey;
use crate::cv_match::match_documents::Uses;
use crate::llm::{self, ANTHROPIC, Effort, LOCAL, Llm, OPENAI};
use crate::server::AppState;

/// Where the page is, and where its forms send the browser back to.
const PAGE: &str = "/account";

/// Shortest key either provider hands out, in characters.
const MIN_KEY_LENGTH: usize = 20;
/// Longest key the form takes, in characters.
const MAX_KEY_LENGTH: usize = 300;
/// Longest model name the form takes, in characters.
const MAX_MODEL_LENGTH: usize = 64;
/// Longest local model address the form takes, in characters.
const MAX_BASE_URL_LENGTH: usize = 300;

/// What the address field suggests: Ollama's OpenAI-compatible API.
const BASE_URL_PLACEHOLDER: &str = "http://localhost:11434/v1";

/// The check's instructions and text: the smallest request that proves the key, model and
/// address answer.
const CHECK_SYSTEM: &str = "You check that an API key works. Answer with ok set to true.";
const CHECK_USER: &str = "Check.";

/// A provider in the form's list.
pub struct ProviderOption {
    pub value: &'static str,
    pub label: &'static str,
    pub selected: bool,
}

#[derive(Template)]
#[template(path = "account/account.html")]
struct AccountPage {
    flash: Option<String>,
    saved: Option<SavedKey>,
    /// Saved, but no longer in Credential Manager.
    unreadable: bool,
    /// Why Credential Manager couldn't be read, when it couldn't.
    vault_error: Option<String>,
    providers: Vec<ProviderOption>,
    errors: Vec<String>,
    model: String,
    base_url: String,
    model_placeholder: String,
    base_url_placeholder: &'static str,
}

/// The key form's fields.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct KeyForm {
    provider: String,
    model: String,
    api_key: String,
    base_url: String,
}

/// The uses form: a ticked box is sent as 1, an unticked one not at all.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UsesForm {
    use_for_resume: Option<String>,
    use_for_application: Option<String>,
    use_for_job_evaluation: Option<String>,
}

/// GET account: the key form, and the saved key with its uses.
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    page(&state, None, Vec::new()).await
}

/// POST account/submit_key {provider, model, api_key, base_url}: saves the key, replacing any
/// saved one. With errors the form comes back with them, and what was typed except the key.
pub async fn submit_key(State(state): State<Arc<AppState>>, Form(form): Form<KeyForm>) -> Response {
    let form = KeyForm {
        provider: form.provider.trim().to_string(),
        model: form.model.trim().to_string(),
        api_key: form.api_key.trim().to_string(),
        base_url: form.base_url.trim().to_string(),
    };
    let errors = validation_errors(&form);
    if !errors.is_empty() {
        return page(&state, Some(&form), errors).await;
    }
    let base_url = if form.provider == LOCAL {
        form.base_url.as_str()
    } else {
        ""
    };
    let saved = account_model::save(
        state.db(),
        &state.key_vault,
        state.user_id,
        &form.provider,
        &form.model,
        base_url,
        &form.api_key,
    )
    .await;
    match saved {
        Ok(()) => state.set_flash("Your key is saved. The AI features will run on it.".to_string()),
        Err(e) => flash_failure(&state, "Your key couldn't be saved", &e),
    }
    Redirect::to(PAGE).into_response()
}

/// POST account/submit_uses {use_for_resume, use_for_application, use_for_job_evaluation}:
/// switches the saved key's uses; a box left unticked is off.
pub async fn submit_uses(State(state): State<Arc<AppState>>, Form(form): Form<UsesForm>) -> Response {
    let ticked = |field: &Option<String>| field.as_deref() == Some("1");
    let on = Uses {
        resume: ticked(&form.use_for_resume),
        application: ticked(&form.use_for_application),
        job_evaluation: ticked(&form.use_for_job_evaluation),
    };
    match account_model::save_uses(state.db(), state.user_id, on).await {
        Ok(()) => state.set_flash("Saved where your key is used.".to_string()),
        Err(e) => flash_failure(&state, "Where your key is used couldn't be saved", &e),
    }
    Redirect::to(PAGE).into_response()
}

/// POST account/submit_delete: deletes the saved key.
pub async fn submit_delete(State(state): State<Arc<AppState>>) -> Response {
    match account_model::delete_key(state.db(), &state.key_vault, state.user_id).await {
        Ok(()) => state.set_flash("Your key is deleted.".to_string()),
        Err(e) => flash_failure(&state, "Your key couldn't be deleted", &e),
    }
    Redirect::to(PAGE).into_response()
}

/// POST account/submit_check: asks the saved key's model for one tiny answer, and says whether
/// it came (not in the PHP app, where the AI match was the check).
pub async fn submit_check(State(state): State<Arc<AppState>>) -> Response {
    let said = check(&state).await.unwrap_or_else(|why| why);
    state.set_flash(said);
    Redirect::to(PAGE).into_response()
}

/// "Your key works: …", or Err with why not; failures are logged.
async fn check(state: &AppState) -> Result<String, String> {
    let key = account_model::key_for(state.db(), &state.key_vault, state.user_id)
        .await
        .map_err(|e| logged("The check couldn't read your key", &e.to_string()))?
        .ok_or_else(|| "There's no key to check: save one first.".to_string())?;
    let llm = Llm::use_key(&key.provider, &key.api_key, &key.model, &key.base_url)
        .map_err(|e| logged("The check failed", &e.details()))?;
    let schema = json!({
        "type": "object",
        "properties": { "ok": { "type": "boolean" } },
        "required": ["ok"],
        "additionalProperties": false,
    });
    llm.structured(CHECK_SYSTEM, CHECK_USER, &schema, Effort::Low)
        .await
        .map_err(|e| {
            eprintln!("account/submit_check: {}", e.details());
            format!("The check failed: {e}")
        })?;
    let provider = account_model::provider_label(&key.provider);
    Ok(format!("Your key works: {provider} answered on {}.", llm.model()))
}

/// The page, with the form as typed (None: as saved) and its errors.
async fn page(state: &AppState, typed: Option<&KeyForm>, errors: Vec<String>) -> Response {
    let saved = match account_model::saved(state.db(), state.user_id).await {
        Ok(saved) => saved,
        Err(e) => return failure_page(&format!("The database couldn't answer: {e}")),
    };
    let (unreadable, vault_error) = readable(state, saved.as_ref());
    let provider = typed
        .map(|form| form.provider.as_str())
        .filter(|provider| !provider.is_empty())
        .or(saved.as_ref().map(|saved| saved.provider.as_str()))
        .unwrap_or(OPENAI);
    let saved_field = |field: fn(&Saved) -> &str| saved.as_ref().map(field).unwrap_or("").to_string();
    let page = AccountPage {
        flash: state.take_flash(),
        unreadable,
        vault_error,
        providers: PROVIDERS
            .iter()
            .map(|&(value, label)| ProviderOption {
                value,
                label,
                selected: value == provider,
            })
            .collect(),
        errors,
        model: typed.map_or_else(|| saved_field(|saved| &saved.model), |form| form.model.clone()),
        base_url: typed.map_or_else(|| saved_field(|saved| &saved.base_url), |form| form.base_url.clone()),
        model_placeholder: model_placeholder(),
        base_url_placeholder: BASE_URL_PLACEHOLDER,
        saved: saved.as_ref().map(SavedKey::from),
    };
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => failure_page(&format!("The page couldn't be printed: {e}")),
    }
}

/// Whether a saved key is gone from Credential Manager, and why Credential Manager couldn't be
/// read, when it couldn't.
fn readable(state: &AppState, saved: Option<&Saved>) -> (bool, Option<String>) {
    let Some(saved) = saved.filter(|saved| !saved.key_hint.is_empty()) else {
        return (false, None);
    };
    match state.key_vault.key_for(state.user_id) {
        Ok(key) => (key.is_none(), None),
        Err(e) => {
            eprintln!("account: {e} (saved {} key)", saved.provider);
            (false, Some(e))
        }
    }
}

/// "Default: gpt-5.4-mini or claude-sonnet-5-5".
fn model_placeholder() -> String {
    let models: Vec<&str> = llm::USER_DEFAULT_MODELS.iter().map(|(_, model)| *model).collect();
    format!("Default: {}", models.join(" or "))
}

/// The form's errors, in Trongate validation's words; none when it can be saved.
fn validation_errors(form: &KeyForm) -> Vec<String> {
    let mut errors = Vec::new();
    if !PROVIDERS.iter().any(|(name, _)| *name == form.provider) {
        errors.push("Pick OpenAI, Anthropic or a local model.".to_string());
    }
    errors.extend(model_errors(form));
    errors.extend(key_errors(form));
    if form.provider == LOCAL {
        errors.extend(base_url_errors(&form.base_url));
    }
    errors
}

fn model_errors(form: &KeyForm) -> Vec<String> {
    let model = &form.model;
    if model.is_empty() && form.provider == LOCAL {
        return vec!["A local model needs its model name, like llama3.1:8b.".to_string()];
    }
    if model.chars().count() > MAX_MODEL_LENGTH {
        return vec![format!(
            "The model field cannot exceed {MAX_MODEL_LENGTH} characters in length."
        )];
    }
    if !model.chars().all(is_model_name_character) {
        return vec!["A model name is letters, digits, dots and dashes, like gpt-5.4-mini.".to_string()];
    }
    Vec::new()
}

/// Letters, digits and . _ : / - (PHP's [A-Za-z0-9._:\/-]).
fn is_model_name_character(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._:/-".contains(c)
}

/// The key: required for OpenAI and Anthropic, and looking like one of theirs; optional for a
/// local model.
fn key_errors(form: &KeyForm) -> Vec<String> {
    let key = &form.api_key;
    let length = key.chars().count();
    let needs_key = form.provider != LOCAL;
    if key.is_empty() {
        return if needs_key {
            vec!["The API key field is required.".to_string()]
        } else {
            Vec::new()
        };
    }
    if key.chars().any(char::is_whitespace) {
        return vec!["The API key has spaces in it. Paste only the key.".to_string()];
    }
    if length > MAX_KEY_LENGTH {
        return vec![format!(
            "The API key field cannot exceed {MAX_KEY_LENGTH} characters in length."
        )];
    }
    if !needs_key {
        return Vec::new();
    }
    if length < MIN_KEY_LENGTH {
        return vec![format!(
            "The API key field must be at least {MIN_KEY_LENGTH} characters in length."
        )];
    }
    let prefix = if form.provider == ANTHROPIC { "sk-ant-" } else { "sk-" };
    if !key.starts_with(prefix) {
        return vec![format!(
            "That doesn't look like a key of this provider; it should start with {prefix}."
        )];
    }
    Vec::new()
}

fn base_url_errors(base_url: &str) -> Vec<String> {
    let is_web_address = (base_url.starts_with("http://") || base_url.starts_with("https://"))
        && reqwest::Url::parse(base_url).is_ok_and(|url| url.host_str().is_some());
    if base_url.chars().count() > MAX_BASE_URL_LENGTH {
        return vec![format!(
            "The address field cannot exceed {MAX_BASE_URL_LENGTH} characters in length."
        )];
    }
    if !is_web_address {
        return vec![format!(
            "A local model needs the address of its OpenAI-compatible API, like {BASE_URL_PLACEHOLDER}."
        )];
    }
    Vec::new()
}

/// Logs what failed and returns the message the page shows.
fn logged(what: &str, error: &str) -> String {
    eprintln!("account: {what}: {error}");
    format!("{what}: {error}")
}

fn flash_failure(state: &AppState, what: &str, error: &dyn std::fmt::Display) {
    state.set_flash(logged(what, &error.to_string()));
}

/// The page when it can't be printed: what went wrong, logged too.
fn failure_page(message: &str) -> Response {
    eprintln!("account: {message}");
    (StatusCode::INTERNAL_SERVER_ERROR, message.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(provider: &str, model: &str, api_key: &str, base_url: &str) -> KeyForm {
        KeyForm {
            provider: provider.to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
        }
    }

    #[test]
    fn a_key_must_look_like_its_providers() {
        assert!(validation_errors(&form(OPENAI, "", "sk-proj-0123456789abcdef", "")).is_empty());
        assert_eq!(
            validation_errors(&form(ANTHROPIC, "", "sk-proj-0123456789abcdef", "")),
            ["That doesn't look like a key of this provider; it should start with sk-ant-."]
        );
        assert_eq!(
            validation_errors(&form(OPENAI, "", "", "")),
            ["The API key field is required."]
        );
        assert_eq!(
            validation_errors(&form(OPENAI, "", "sk-short", "")),
            ["The API key field must be at least 20 characters in length."]
        );
        assert_eq!(
            validation_errors(&form(OPENAI, "gpt 5", "sk-proj-0123456789 abcdef", "")),
            [
                "A model name is letters, digits, dots and dashes, like gpt-5.4-mini.",
                "The API key has spaces in it. Paste only the key."
            ]
        );
        assert_eq!(
            validation_errors(&form("gemini", "", "sk-proj-0123456789abcdef", "")),
            ["Pick OpenAI, Anthropic or a local model."]
        );
    }

    #[test]
    fn a_local_model_needs_a_name_and_an_address_but_no_key() {
        assert!(validation_errors(&form(LOCAL, "llama3.1:8b", "", "http://localhost:11434/v1")).is_empty());
        assert!(validation_errors(&form(LOCAL, "qwen3", "lm-studio", "http://127.0.0.1:1234/v1")).is_empty());
        assert_eq!(
            validation_errors(&form(LOCAL, "", "", "localhost:11434")),
            [
                "A local model needs its model name, like llama3.1:8b.",
                "A local model needs the address of its OpenAI-compatible API, like http://localhost:11434/v1."
            ]
        );
        // Only a local model reads the address.
        assert!(validation_errors(&form(OPENAI, "", "sk-proj-0123456789abcdef", "nonsense")).is_empty());
    }

    #[test]
    fn the_model_field_names_the_default_models() {
        assert_eq!(model_placeholder(), "Default: gpt-5.4-mini or claude-sonnet-5-5");
    }
}
