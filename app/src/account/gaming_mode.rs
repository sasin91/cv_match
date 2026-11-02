//! Gaming mode: while the user plays (user_busy), work that would run on a model on this
//! computer (this app's llama-server, or the user's own local server) runs on their cloud key
//! instead, so the game keeps the graphics card: their OpenAI key, else their Anthropic key, on
//! that key's own model. llama-server lets go of the card as soon as no call needs it (idle).
//! Once the game ends, the next call runs on this computer again.
//!
//! Without a cloud key, or with gaming mode off (user_gaming_mode), nothing moves: the job scan
//! waits while the game runs, as it did before gaming mode. It is on until the user turns it
//! off.

use sqlx::SqlitePool;

use super::account_model::{self, KeyFor, Saved};
use crate::context::Context;
use crate::llm::{ANTHROPIC, LLAMA_CPP, LOCAL, OPENAI};
use crate::service_error::ServiceError;
use crate::user_busy::{self, Busy};

/// The cloud keys gaming mode moves work to, the first one saved first: OpenAI's before
/// Anthropic's, as the app's default is OpenAI's.
const CLOUD_PROVIDERS: [&str; 2] = [OPENAI, ANTHROPIC];

/// The providers whose model runs on this computer.
const ON_THIS_COMPUTER: [&str; 2] = [LLAMA_CPP, LOCAL];

/// Work moved to a cloud key while the user plays.
pub struct StandIn {
    /// The cloud key, on its own model.
    pub key: KeyFor,
    /// Why the user counts as playing.
    pub busy: Busy,
}

/// Gaming mode as the Language models screen shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct GamingModeView {
    pub on: bool,
    /// The cloud key the work moves to ("OpenAI"); None without one.
    pub cloud_key: Option<String>,
}

/// Gaming mode for the screen, given the saved keys and whether it's on; None when no saved key
/// runs on this computer, so there's nothing to move.
pub fn view(saved: &[Saved], on: bool) -> Option<GamingModeView> {
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
    Some(GamingModeView { on, cloud_key })
}

/// Whether gaming mode is on for the user: on unless they turned it off.
pub async fn is_on(db: &SqlitePool, user_id: i64) -> sqlx::Result<bool> {
    let offload = sqlx::query_scalar!(
        r#"SELECT offload FROM user_gaming_mode WHERE trongate_user_id = ?1"#,
        user_id
    )
    .fetch_optional(db)
    .await?;
    Ok(offload.is_none_or(|offload| offload != 0))
}

/// Turns gaming mode on or off for the user.
pub async fn set_on(db: &SqlitePool, user_id: i64, on: bool) -> sqlx::Result<()> {
    let offload = i64::from(on);
    let mut tx = crate::db::write_transaction(db).await?;
    sqlx::query!(
        r#"
        INSERT INTO user_gaming_mode (trongate_user_id, offload) VALUES (?1, ?2)
        ON CONFLICT (trongate_user_id) DO UPDATE SET offload = excluded.offload
        "#,
        user_id,
        offload
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// The cloud key that work on key runs on instead while the user plays, or None: key runs in
/// the cloud already, the user isn't playing, gaming mode is off, or there's no cloud key.
pub async fn stand_in_for(context: &Context, key: &KeyFor) -> Result<Option<StandIn>, ServiceError> {
    if !ON_THIS_COMPUTER.contains(&key.provider.as_str()) {
        return Ok(None);
    }
    stand_in_now(context).await
}

/// The cloud key this computer's work runs on now, or None: the user isn't playing, gaming mode
/// is off, or there's no cloud key.
pub async fn stand_in_now(context: &Context) -> Result<Option<StandIn>, ServiceError> {
    let Some(busy) = user_busy::why_busy() else {
        return Ok(None);
    };
    if !is_on(context.db(), context.user_id).await? {
        return Ok(None);
    }
    Ok(cloud_key(context).await?.map(|key| StandIn { key, busy }))
}

/// The user's first saved cloud key (CLOUD_PROVIDERS) that is in Credential Manager, on its own
/// model.
pub async fn cloud_key(context: &Context) -> Result<Option<KeyFor>, ServiceError> {
    for provider in CLOUD_PROVIDERS {
        let key = account_model::key_of(context.db(), &context.key_vault, context.user_id, provider)
            .await
            .map_err(|e| {
                crate::log_line!("gaming mode: the {provider} key couldn't be read: {e}");
                ServiceError::internal("gaming mode", format!("Your API key couldn't be read: {e}"))
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
    use crate::db;

    async fn with_user() -> SqlitePool {
        let db = db::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO trongate_users (id) VALUES (1)")
            .execute(&db)
            .await
            .unwrap();
        db
    }

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
    fn the_screen_shows_gaming_mode_only_with_a_model_on_this_computer() {
        assert_eq!(view(&[saved(OPENAI)], true), None);
        let local_only = view(&[saved(LLAMA_CPP)], true).unwrap();
        assert_eq!(local_only.cloud_key, None);
    }

    #[test]
    fn the_work_moves_to_openai_before_claude() {
        let both = view(&[saved(ANTHROPIC), saved(LLAMA_CPP), saved(OPENAI)], false).unwrap();
        assert_eq!(both.cloud_key.as_deref(), Some(account_model::provider_label(OPENAI)));
        assert!(!both.on);
        let claude = view(&[saved(LOCAL), saved(ANTHROPIC)], true).unwrap();
        assert_eq!(
            claude.cloud_key.as_deref(),
            Some(account_model::provider_label(ANTHROPIC))
        );
    }

    #[tokio::test]
    async fn gaming_mode_is_on_until_turned_off() {
        let db = with_user().await;
        assert!(is_on(&db, 1).await.unwrap());
        set_on(&db, 1, false).await.unwrap();
        assert!(!is_on(&db, 1).await.unwrap());
        set_on(&db, 1, true).await.unwrap();
        assert!(is_on(&db, 1).await.unwrap());
    }
}
