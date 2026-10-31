//! account: the API key page for the one local user.
//! The AI features of the CV match run on the user's own OpenAI or Anthropic API keys, or on a
//! local OpenAI-compatible server (Ollama, LM Studio), or on a GGUF model the app runs itself with
//! llama.cpp (the llama module), which needs no key. One key is kept per provider, in Windows
//! Credential Manager, and never shown again; only its last 4 characters are. Each use (tailoring
//! résumés, writing applications, evaluating job posts) runs on the key picked for it, or on none.
//!
//! The work is in account_service; the handlers here read the form, call it and print the page.
//! There is no account on the desktop, so no profile links, data download or account deletion.

pub mod account_model;
pub mod account_service;
pub mod key_vault;
pub mod llama_panel;
pub mod llama_setup;
pub mod llama_setup_service;
pub mod model_advice;
pub mod saved_key;

use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use self::account_model::UseChoices;
use self::account_service::{AccountView, KeyForm, ProviderOption, SaveKey};
use self::llama_panel::LlamaPanel;
use self::model_advice::ModelAdvice;
use self::saved_key::{KeyUse, SavedKey};
use crate::server::AppState;
use crate::setup;

/// Where the page is, and where its forms send the browser back to.
const PAGE: &str = "/account";

#[derive(Template)]
#[template(path = "account/account.html")]
struct AccountPage {
    flash: Option<String>,
    saved: Vec<SavedKey>,
    /// Per use, the list that picks its key; empty without saved keys.
    key_uses: Vec<KeyUse>,
    /// Why Credential Manager couldn't be read, when it couldn't.
    vault_error: Option<String>,
    providers: Vec<ProviderOption>,
    errors: Vec<String>,
    model: String,
    base_url: String,
    model_placeholder: String,
    base_url_placeholder: &'static str,
    /// This app's own model, when the app has a llama folder.
    llama: Option<LlamaPanel>,
    /// Which model to run the AI on, shown while no key is saved.
    advice: ModelAdvice,
    /// The setup wizard isn't finished, so the page links back to it.
    setup_unfinished: bool,
}

/// The uses form: per use, the provider whose key runs it, or "" for none.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct UsesForm {
    use_for_resume: String,
    use_for_application: String,
    use_for_job_evaluation: String,
}

/// The check and delete forms: the saved key's provider.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProviderForm {
    provider: String,
}

/// GET account: the key form, and the saved keys with the uses they run.
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    page(&state, None, Vec::new()).await
}

/// POST account/submit_key {provider, model, api_key, base_url}: saves the provider's key,
/// replacing the one saved for that provider; other providers' keys stay. With errors the form
/// comes back with them, and what was typed except the key.
pub async fn submit_key(State(state): State<Arc<AppState>>, Form(form): Form<KeyForm>) -> Response {
    match account_service::save_key(&state.core, &form).await {
        Ok(SaveKey::Invalid { form, errors }) => page(&state, Some(&form), errors).await,
        Ok(SaveKey::Saved {
            message,
            setup_unfinished,
        }) => {
            state.set_flash(message);
            Redirect::to(if setup_unfinished { setup::PAGE } else { PAGE }).into_response()
        }
        Err(e) => {
            state.set_flash(e.message);
            Redirect::to(PAGE).into_response()
        }
    }
}

/// POST account/submit_uses {use_for_resume, use_for_application, use_for_job_evaluation}: runs
/// each use on the key of the provider sent for it; "" runs it on none.
pub async fn submit_uses(State(state): State<Arc<AppState>>, Form(form): Form<UsesForm>) -> Response {
    let picked = |provider: String| Some(provider).filter(|provider| !provider.is_empty());
    let chosen = UseChoices {
        resume: picked(form.use_for_resume),
        application: picked(form.use_for_application),
        job_evaluation: picked(form.use_for_job_evaluation),
    };
    let said = account_service::save_uses(&state.core, &chosen).await;
    state.set_flash(said.map_or_else(|e| e.message, str::to_string));
    Redirect::to(PAGE).into_response()
}

/// POST account/submit_delete {provider}: deletes that provider's saved key.
pub async fn submit_delete(State(state): State<Arc<AppState>>, Form(form): Form<ProviderForm>) -> Response {
    let said = account_service::delete_key(&state.core, &form.provider).await;
    state.set_flash(said.unwrap_or_else(|e| e.message));
    Redirect::to(PAGE).into_response()
}

/// POST account/submit_check {provider}: asks that provider's saved key's model for one tiny
/// answer, and says whether it came.
pub async fn submit_check(State(state): State<Arc<AppState>>, Form(form): Form<ProviderForm>) -> Response {
    let said = account_service::check_key(&state.core, &form.provider).await;
    state.set_flash(said.unwrap_or_else(|e| e.message));
    Redirect::to(PAGE).into_response()
}

/// The page, with the form as typed (None: a provider without a saved key, if any) and its
/// errors.
async fn page(state: &AppState, typed: Option<&KeyForm>, errors: Vec<String>) -> Response {
    let view = match account_service::account_view(&state.core, typed).await {
        Ok(view) => view,
        // Logged when it was made.
        Err(e) => return failure_page(e.message),
    };
    let AccountView {
        saved,
        key_uses,
        vault_error,
        providers,
        model,
        base_url,
        model_placeholder,
        base_url_placeholder,
        llama,
        advice,
        setup_unfinished,
    } = view;
    let page = AccountPage {
        flash: state.take_flash(),
        saved,
        key_uses,
        vault_error,
        providers,
        errors,
        model,
        base_url,
        model_placeholder,
        base_url_placeholder,
        llama,
        advice,
        setup_unfinished,
    };
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("account: the page couldn't be printed: {e}");
            failure_page(format!("The page couldn't be printed: {e}"))
        }
    }
}

/// The page when it can't be printed: what went wrong.
fn failure_page(message: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
}
