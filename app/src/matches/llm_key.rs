//! Which language model a match endpoint runs on: the user's own key from the API key page,
//! picked for the use (account_model::USES). The desktop app has no server key to
//! fall back to, so without a key for that use the page is told why and pointed to the API key
//! page.

use super::match_documents::Uses;
use crate::account::account_model::{self, APPLICATION, JOB_EVALUATION, KeyFor, RESUME, USES};
use crate::context::Context;
use crate::llama;
use crate::llm::Llm;
use crate::service_error::{ErrorKind, ServiceError};

/// A model to call, and who it is as the documents it writes record it ("openai/gpt-5.4-mini").
pub struct KeyedLlm {
    pub llm: Llm,
    pub written_by: String,
}

/// The user's key settings for the key picked for use, else None.
pub async fn llm_settings(context: &Context, use_: &str) -> Result<Option<KeyFor>, ServiceError> {
    key_for(context, use_).await
}

/// For each use, whether there's a key to use for it, for the page.
pub async fn ai_uses(context: &Context) -> Result<Uses, ServiceError> {
    Ok(Uses {
        resume: key_for(context, RESUME).await?.is_some(),
        application: key_for(context, APPLICATION).await?.is_some(),
        job_evaluation: key_for(context, JOB_EVALUATION).await?.is_some(),
    })
}

/// The model on the user's key for use, or the 402 the page answers with a link to the API key
/// page.
pub async fn llm(context: &Context, use_: &str) -> Result<KeyedLlm, ServiceError> {
    let Some(key) = llm_settings(context, use_).await? else {
        return Err(ServiceError::need_key(missing_key_message(context, use_).await?));
    };
    let llm = llama::use_key(context.llama.as_ref(), &key)
        .await
        .map_err(|e| ServiceError::llm(&format!("llm ({use_})"), &e))?;
    let written_by = format!("{}/{}", key.provider, llm.model());
    Ok(KeyedLlm { llm, written_by })
}

/// Why there's no key to use: none picked for this use, the picked one no longer in Windows
/// Credential Manager, or none saved.
async fn missing_key_message(context: &Context, use_: &str) -> Result<String, ServiceError> {
    let saved = account_model::saved(context.db(), context.user_id).await?;
    let message = match account_model::picked_for(&saved, use_) {
        Some(picked) => {
            crate::log_line!(
                "match: user {}'s saved {} key isn't in Windows Credential Manager",
                context.user_id,
                picked.provider
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

async fn key_for(context: &Context, use_: &str) -> Result<Option<KeyFor>, ServiceError> {
    account_model::key_for(context.db(), &context.key_vault, context.user_id, use_)
        .await
        .map_err(|e| {
            crate::log_line!("match: the API key couldn't be read: {e}");
            ServiceError::new(ErrorKind::Internal, format!("Your API key couldn't be read: {e}"))
        })
}
