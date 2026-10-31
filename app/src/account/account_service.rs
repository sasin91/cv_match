//! The API key page's work, outside the web page, so a desktop window can do the same from the
//! same functions: the key form's checks, saving and deleting a provider's key, which key each
//! use runs on, asking a key's model for one tiny answer, and the page's data as a view model.
//! Every answer is a message to show, or a ServiceError (logged when it is the app's failure).

use serde::Deserialize;
use serde_json::json;

use super::account_model::{self, PROVIDERS, Saved, UseChoices};
use super::llama_panel::LlamaPanel;
use super::model_advice::ModelAdvice;
use super::saved_key::{self, KeyUse, SavedKey};
use crate::context::Context;
use crate::llama;
use crate::llm::{self, ANTHROPIC, Effort, LLAMA_CPP, LOCAL, OPENAI};
use crate::service_error::{ErrorKind, ServiceError};
use crate::setup;

/// Shortest key either provider hands out, in characters.
const MIN_KEY_LENGTH: usize = 20;
/// Longest key the form takes, in characters.
const MAX_KEY_LENGTH: usize = 300;
/// Longest model name the form takes, in characters.
const MAX_MODEL_LENGTH: usize = 64;
/// Longest local model address the form takes, in characters.
const MAX_BASE_URL_LENGTH: usize = 300;

/// What the address field suggests: Ollama's OpenAI-compatible API.
pub const BASE_URL_PLACEHOLDER: &str = "http://localhost:11434/v1";

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

/// The key form's fields.
#[derive(Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct KeyForm {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

impl KeyForm {
    /// The form without the spaces around what was typed.
    fn trimmed(&self) -> KeyForm {
        KeyForm {
            provider: self.provider.trim().to_string(),
            model: self.model.trim().to_string(),
            api_key: self.api_key.trim().to_string(),
            base_url: self.base_url.trim().to_string(),
        }
    }
}

/// The API key page's data: the saved keys with the uses they run, and the key form.
pub struct AccountView {
    pub saved: Vec<SavedKey>,
    /// Per use, the list that picks its key; empty without saved keys.
    pub key_uses: Vec<KeyUse>,
    /// Why Credential Manager couldn't be read, when it couldn't.
    pub vault_error: Option<String>,
    pub providers: Vec<ProviderOption>,
    pub model: String,
    pub base_url: String,
    pub model_placeholder: String,
    pub base_url_placeholder: &'static str,
    /// This app's own model, when the app has a llama folder.
    pub llama: Option<LlamaPanel>,
    /// Which model to run the AI on, shown while no key is saved.
    pub advice: ModelAdvice,
    /// The setup wizard isn't finished, so the page links back to it.
    pub setup_unfinished: bool,
}

/// What came of the key form.
#[derive(Debug, PartialEq)]
pub enum SaveKey {
    /// It can't be saved, for these reasons; the form comes back with what was typed except the
    /// key.
    Invalid { form: KeyForm, errors: Vec<String> },
    /// Saved; the setup wizard isn't finished when setup_unfinished, so the user goes back to it.
    Saved { message: String, setup_unfinished: bool },
}

/// Saves the provider's key, replacing the one saved for that provider; other providers' keys
/// stay.
pub async fn save_key(context: &Context, form: &KeyForm) -> Result<SaveKey, ServiceError> {
    let form = form.trimmed();
    let mut errors = validation_errors(&form);
    errors.extend(llama_model_errors(context, &form));
    if !errors.is_empty() {
        return Ok(SaveKey::Invalid { form, errors });
    }
    let base_url = if form.provider == LOCAL {
        form.base_url.as_str()
    } else {
        ""
    };
    // The app's own model has no key to keep.
    let api_key = if form.provider == LLAMA_CPP {
        ""
    } else {
        form.api_key.as_str()
    };
    account_model::save(
        context.db(),
        &context.key_vault,
        context.user_id,
        &form.provider,
        &form.model,
        base_url,
        api_key,
    )
    .await
    .map_err(|e| ServiceError::internal("account/save_key", format!("Your key couldn't be saved: {e}")))?;
    let label = account_model::provider_label(&form.provider);
    let setup_unfinished = setup::first_run_or_logged(context).await;
    let message = if setup_unfinished {
        format!("Your {label} key is saved, for every use.")
    } else {
        format!("Your {label} key is saved. Pick below what it is used for.")
    };
    Ok(SaveKey::Saved {
        message,
        setup_unfinished,
    })
}

/// Runs each use on the key of the provider chosen for it, or on none.
pub async fn save_uses(context: &Context, chosen: &UseChoices) -> Result<&'static str, ServiceError> {
    account_model::save_uses(context.db(), context.user_id, chosen)
        .await
        .map_err(|e| {
            ServiceError::internal(
                "account/save_uses",
                format!("Which key each use runs on couldn't be saved: {e}"),
            )
        })?;
    Ok("Saved which key each use runs on.")
}

/// Deletes that provider's saved key.
pub async fn delete_key(context: &Context, provider: &str) -> Result<String, ServiceError> {
    let label = account_model::provider_label(provider);
    account_model::delete_key(context.db(), &context.key_vault, context.user_id, provider)
        .await
        .map_err(|e| {
            ServiceError::internal(
                "account/delete_key",
                format!("Your {label} key couldn't be deleted: {e}"),
            )
        })?;
    Ok(format!("Your {label} key is deleted."))
}

/// Asks that provider's saved key's model for one tiny answer: "Your key works: …", or why not.
pub async fn check_key(context: &Context, provider: &str) -> Result<String, ServiceError> {
    let key = account_model::key_of(context.db(), &context.key_vault, context.user_id, provider)
        .await
        .map_err(|e| ServiceError::internal("account/check_key", format!("The check couldn't read your key: {e}")))?
        .ok_or_else(|| ServiceError::need_key("There's no key to check: save one first."))?;
    let llm = llama::use_key(context.llama.as_ref(), &key)
        .await
        .map_err(|e| ServiceError::failed("account/check_key", format!("The check failed: {}", e.details())))?;
    let schema = json!({
        "type": "object",
        "properties": { "ok": { "type": "boolean" } },
        "required": ["ok"],
        "additionalProperties": false,
    });
    llm.structured(CHECK_SYSTEM, CHECK_USER, &schema, Effort::Low)
        .await
        .map_err(|e| {
            crate::log_line!("account/check_key: {}", e.details());
            ServiceError::new(
                ErrorKind::Llm { retryable: e.retryable },
                format!("The check failed: {e}"),
            )
        })?;
    let label = account_model::provider_label(&key.provider);
    Ok(format!("Your key works: {label} answered on {}.", llm.model()))
}

/// The page's data, with the form as typed (None: a provider without a saved key, if any).
pub async fn account_view(context: &Context, typed: Option<&KeyForm>) -> Result<AccountView, ServiceError> {
    let together = context.key_vault.lock().await;
    let saved = account_model::saved(context.db(), context.user_id).await?;
    let (saved_keys, vault_error) = readable(context, &saved);
    drop(together);
    let setup_unfinished = setup::first_run_or_logged(context).await;
    let advice = ModelAdvice::from(context.llama.as_ref());
    let provider = typed
        .map(|form| form.provider.as_str())
        .filter(|provider| !provider.is_empty())
        .unwrap_or_else(|| provider_to_add(&saved, advice.local_first()));
    let saved_of_provider = saved.iter().find(|saved| saved.provider == provider);
    let saved_field = |field: fn(&Saved) -> &str| saved_of_provider.map(field).unwrap_or("").to_string();
    Ok(AccountView {
        key_uses: if saved.is_empty() {
            Vec::new()
        } else {
            saved_key::key_uses(&saved)
        },
        saved: saved_keys,
        vault_error,
        providers: PROVIDERS
            .iter()
            .map(|&(value, label)| ProviderOption {
                value,
                label,
                selected: value == provider,
            })
            .collect(),
        model: typed.map_or_else(|| saved_field(|saved| &saved.model), |form| form.model.clone()),
        base_url: typed.map_or_else(|| saved_field(|saved| &saved.base_url), |form| form.base_url.clone()),
        model_placeholder: model_placeholder(),
        base_url_placeholder: BASE_URL_PLACEHOLDER,
        llama: context.llama.as_ref().map(|server| LlamaPanel::from(server.overview())),
        advice,
        setup_unfinished,
    })
}

/// The providers in the order the form suggests them: this app's own model first when this
/// computer has room for a good one (model_advice), else OpenAI first. Never Anthropic first.
fn suggestion_order(local_first: bool) -> [&'static str; PROVIDERS.len()] {
    if local_first {
        [LLAMA_CPP, OPENAI, ANTHROPIC, LOCAL]
    } else {
        [OPENAI, ANTHROPIC, LOCAL, LLAMA_CPP]
    }
}

/// The first provider without a saved key in suggestion_order, for the form to add; OpenAI when
/// all have one.
fn provider_to_add(saved: &[Saved], local_first: bool) -> &'static str {
    suggestion_order(local_first)
        .into_iter()
        .find(|name| !saved.iter().any(|saved| saved.provider == *name))
        .unwrap_or(OPENAI)
}

/// The saved keys for the page, each saying whether it is gone from Credential Manager, and why
/// Credential Manager couldn't be read, when it couldn't. The caller holds the vault's lock.
fn readable(context: &Context, saved: &[Saved]) -> (Vec<SavedKey>, Option<String>) {
    let mut vault_error = None;
    let keys = saved
        .iter()
        .map(|saved| {
            let unreadable = match account_model::with_key(&context.key_vault, context.user_id, saved) {
                Ok(key) => key.is_none(),
                Err(e) => {
                    crate::log_line!("account: {e} (saved {} key)", saved.provider);
                    vault_error.get_or_insert(e.to_string());
                    false
                }
            };
            SavedKey::from(saved, unreadable)
        })
        .collect();
    (keys, vault_error)
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
        errors.push("Pick OpenAI, Anthropic, a local model or this app's own model.".to_string());
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

/// Letters, digits and . _ : / -.
fn is_model_name_character(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._:/-".contains(c)
}

/// The key: required for OpenAI and Anthropic, and looking like one of theirs; optional for a
/// local model.
fn key_errors(form: &KeyForm) -> Vec<String> {
    let key = &form.api_key;
    let length = key.chars().count();
    let needs_key = form.provider != LOCAL && form.provider != LLAMA_CPP;
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

/// For this app's own model: a named model must be a file in the models folder. Blank is the one
/// that fits best.
fn llama_model_errors(context: &Context, form: &KeyForm) -> Vec<String> {
    if form.provider != LLAMA_CPP {
        return Vec::new();
    }
    let Some(server) = &context.llama else {
        return vec!["The app was started without its llama folder.".to_string()];
    };
    let named = &form.model;
    let found = named.is_empty()
        || server
            .models()
            .iter()
            .any(|model| model.name.eq_ignore_ascii_case(named));
    if found {
        return Vec::new();
    }
    vec![format!(
        "There's no {named} in {}. Name a .gguf file there, or leave the model blank.",
        server.models_folder().display()
    )]
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
            ["Pick OpenAI, Anthropic, a local model or this app's own model."]
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
    fn this_apps_own_model_needs_no_key_name_or_address() {
        assert!(validation_errors(&form(LLAMA_CPP, "", "", "")).is_empty());
        assert!(validation_errors(&form(LLAMA_CPP, "Qwen3-8B-Q4_K_M.gguf", "", "")).is_empty());
    }

    #[test]
    fn the_form_suggests_the_apps_own_model_only_when_it_fits_and_never_anthropic_first() {
        assert_eq!(provider_to_add(&[], true), LLAMA_CPP);
        assert_eq!(provider_to_add(&[], false), OPENAI);
        let saved = |provider: &str| Saved {
            provider: provider.to_string(),
            model: String::new(),
            base_url: String::new(),
            key_hint: String::new(),
            uses: Default::default(),
            updated_at: 0,
        };
        assert_eq!(provider_to_add(&[saved(LLAMA_CPP)], true), OPENAI);
        assert_eq!(provider_to_add(&[saved(OPENAI)], false), ANTHROPIC);
    }

    #[test]
    fn the_model_field_names_the_default_models() {
        assert_eq!(model_placeholder(), "Default: gpt-5.4-mini or claude-sonnet-5-5");
    }
}
