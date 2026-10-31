//! Which language model a match endpoint runs on: the user's own key from the API key page,
//! picked for the use (account_model::USES). The desktop app has no server key to
//! fall back to, so without a key for that use the page is told why and pointed to the API key
//! page.

use super::match_documents::Uses;
use super::page_error::PageError;
use crate::account::account_model::{self, APPLICATION, JOB_EVALUATION, KeyFor, RESUME, USES};
use crate::llama;
use crate::llm::Llm;
use crate::server::AppState;

/// A model to call, and who it is as the documents it writes record it ("openai/gpt-5.4-mini").
pub struct KeyedLlm {
    pub llm: Llm,
    pub written_by: String,
}

/// The user's key settings for the key picked for use, else None.
pub async fn llm_settings(state: &AppState, use_: &str) -> Result<Option<KeyFor>, PageError> {
    key_for(state, use_).await
}

/// For each use, whether there's a key to use for it, for the page.
pub async fn ai_uses(state: &AppState) -> Result<Uses, PageError> {
    Ok(Uses {
        resume: key_for(state, RESUME).await?.is_some(),
        application: key_for(state, APPLICATION).await?.is_some(),
        job_evaluation: key_for(state, JOB_EVALUATION).await?.is_some(),
    })
}

/// The model on the user's key for use, or the 402 the page answers with a link to the API key
/// page.
pub async fn llm(state: &AppState, use_: &str) -> Result<KeyedLlm, PageError> {
    let Some(key) = llm_settings(state, use_).await? else {
        return Err(PageError::need_key(missing_key_message(state, use_).await?));
    };
    let llm = llama::use_key(state.llama.as_ref(), &key)
        .await
        .map_err(|e| PageError::llm(use_, &e))?;
    let written_by = format!("{}/{}", key.provider, llm.model());
    Ok(KeyedLlm { llm, written_by })
}

/// Why there's no key to use: none picked for this use, the picked one no longer in Windows
/// Credential Manager, or none saved.
async fn missing_key_message(state: &AppState, use_: &str) -> Result<String, PageError> {
    let saved = account_model::saved(state.db(), state.user_id).await?;
    let message = match account_model::picked_for(&saved, use_) {
        Some(picked) => {
            eprintln!(
                "match: user {}'s saved {} key isn't in Windows Credential Manager",
                state.user_id, picked.provider
            );
            format!(
                "Your saved {} key isn't in Windows Credential Manager anymore. Save it again on the API key page.",
                account_model::provider_label(&picked.provider)
            )
        }
        None if !saved.is_empty() => {
            let label = USES
                .iter()
                .find(|(name, _)| *name == use_)
                .map(|(_, label)| lowercase_first(label))
                .unwrap_or_default();
            format!("None of your API keys is picked for {label}. Pick one on the API key page.")
        }
        None => "Add your OpenAI or Anthropic API key, or a local model, on the API key page to use the AI match."
            .to_string(),
    };
    Ok(message)
}

/// "Tailoring résumés" as "tailoring résumés", keeping "AI" in "(the AI match)".
fn lowercase_first(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

async fn key_for(state: &AppState, use_: &str) -> Result<Option<KeyFor>, PageError> {
    account_model::key_for(state.db(), &state.key_vault, state.user_id, use_)
        .await
        .map_err(|e| {
            eprintln!("match: the API key couldn't be read: {e}");
            PageError::new(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("Your API key couldn't be read: {e}"),
            )
        })
}
