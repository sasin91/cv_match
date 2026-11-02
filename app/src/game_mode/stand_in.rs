//! The cloud key that stands in while a game runs: work that would run on a model on this
//! computer (this app's llama-server, or the user's own local server) runs on their OpenAI key,
//! else their Anthropic key, on that key's own model, so the game keeps the graphics card.
//! llama-server lets go of the card as soon as no call needs it (idle). Once the game ends, the
//! next call runs on this computer again.
//!
//! Without a cloud key nothing moves: the job scan waits while the game runs.

use super::Playing;
use crate::account::account_model::{self, KeyFor, Saved};
use crate::context::Context;
use crate::llm::{ANTHROPIC, LLAMA_CPP, LOCAL, OPENAI};
use crate::service_error::ServiceError;

/// The cloud keys work moves to, the first one saved first: OpenAI's before Anthropic's, as the
/// app's default is OpenAI's.
const CLOUD_PROVIDERS: [&str; 2] = [OPENAI, ANTHROPIC];

/// The providers whose model runs on this computer.
const ON_THIS_COMPUTER: [&str; 2] = [LLAMA_CPP, LOCAL];

/// Work moved to a cloud key while a game runs.
pub struct StandIn {
    /// The cloud key, on its own model.
    pub key: KeyFor,
    /// Why a game counts as running.
    pub playing: Playing,
}

/// What the Language models screen says about games, when a saved key runs on this computer.
#[derive(Debug, Clone, PartialEq)]
pub struct WhilePlaying {
    /// The cloud key the work moves to ("OpenAI"); None without one.
    pub cloud_key: Option<String>,
}

/// What the screen says, given the saved keys; None when no saved key runs on this computer,
/// so there's nothing to move.
pub fn while_playing(saved: &[Saved]) -> Option<WhilePlaying> {
    let runs_here = saved
        .iter()
        .any(|saved| ON_THIS_COMPUTER.contains(&saved.provider.as_str()));
    if !runs_here {
        return None;
    }
    let cloud_key = CLOUD_PROVIDERS
        .into_iter()
        .find(|provider| saved.iter().any(|saved| saved.provider == *provider))
        .map(|provider| account_model::provider_label(provider).to_string());
    Some(WhilePlaying { cloud_key })
}

/// The cloud key that work on key runs on instead while a game runs, or None: key runs in the
/// cloud already, no game runs, or there's no cloud key.
pub async fn for_key(context: &Context, key: &KeyFor) -> Result<Option<StandIn>, ServiceError> {
    if !ON_THIS_COMPUTER.contains(&key.provider.as_str()) {
        return Ok(None);
    }
    now(context).await
}

/// The cloud key this computer's work runs on now, or None: no game runs, or there's no cloud
/// key.
pub async fn now(context: &Context) -> Result<Option<StandIn>, ServiceError> {
    let Some(playing) = context.game_mode.why_running() else {
        return Ok(None);
    };
    Ok(cloud_key(context).await?.map(|key| StandIn { key, playing }))
}

/// The user's first saved cloud key (CLOUD_PROVIDERS) that is in Credential Manager, on its own
/// model.
async fn cloud_key(context: &Context) -> Result<Option<KeyFor>, ServiceError> {
    for provider in CLOUD_PROVIDERS {
        let key = account_model::key_of(context.db(), &context.key_vault, context.user_id, provider)
            .await
            .map_err(|e| {
                crate::log_line!("game mode: the {provider} key couldn't be read: {e}");
                ServiceError::internal("game mode", format!("Your API key couldn't be read: {e}"))
            })?;
        if let Some(key) = key.filter(|key| !key.api_key.is_empty()) {
            return Ok(Some(key));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(provider: &str) -> Saved {
        Saved {
            provider: provider.to_string(),
            model: String::new(),
            base_url: String::new(),
            key_hint: String::new(),
            uses: Default::default(),
            updated_at: 0,
        }
    }

    #[test]
    fn the_screen_says_it_only_with_a_model_on_this_computer() {
        assert_eq!(while_playing(&[saved(OPENAI)]), None);
        assert_eq!(
            while_playing(&[saved(LLAMA_CPP)]),
            Some(WhilePlaying { cloud_key: None })
        );
    }

    #[test]
    fn the_work_moves_to_openai_before_claude() {
        let both = while_playing(&[saved(ANTHROPIC), saved(LLAMA_CPP), saved(OPENAI)]).unwrap();
        assert_eq!(both.cloud_key.as_deref(), Some(account_model::provider_label(OPENAI)));
        let claude = while_playing(&[saved(LOCAL), saved(ANTHROPIC)]).unwrap();
        assert_eq!(
            claude.cloud_key.as_deref(),
            Some(account_model::provider_label(ANTHROPIC))
        );
    }
}
