//! The user's own language model keys (user_llm_keys), one per provider, and which one each use
//! runs on (use_for_*: each use is on for one key at most). The keys themselves are in Windows
//! Credential Manager (KeyVault); the table keeps each one's provider, model, last 4 characters
//! and, for a local model, base_url. A plain key only leaves here through key_for() and
//! key_of(), for the llm module.

use std::fmt;

use sqlx::SqlitePool;

use super::key_vault::KeyVault;
use crate::llm::{ANTHROPIC, LLAMA_CPP, LOCAL, OPENAI};
use crate::matches::match_documents::Uses;

/// provider => what the page calls it.
pub const PROVIDERS: [(&str, &str); 4] = [
    (OPENAI, "OpenAI"),
    (ANTHROPIC, "Anthropic (Claude)"),
    (LOCAL, "A local model (Ollama, LM Studio)"),
    (LLAMA_CPP, "This app's own model (llama.cpp)"),
];

/// What a key can be used for, each run on the key the user picks for it, or on none: purpose =>
/// what the page calls it. A new key takes the uses no other key has.
pub const USES: [(&str, &str); 3] = [
    (RESUME, "Tailoring résumés"),
    (APPLICATION, "Writing job applications"),
    (JOB_EVALUATION, "Evaluating job posts (the AI match)"),
];
pub use crate::matches::document_box::{APPLICATION, RESUME};
pub const JOB_EVALUATION: &str = "job_evaluation";

/// How many of the key's last characters the page shows.
const KEY_HINT_CHARACTERS: usize = 4;

/// A saved key's details without the key.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// The key's last characters; "" for a local model saved without a key.
    pub key_hint: String,
    pub uses: Uses,
    pub updated_at: i64,
}

/// { provider, model, base_url, api_key, uses } for the llm module.
#[derive(Clone, PartialEq)]
pub struct KeyFor {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub uses: Uses,
}

impl fmt::Debug for KeyFor {
    /// Everything but the key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyFor")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("uses", &self.uses)
            .finish_non_exhaustive()
    }
}

/// For each use, the provider whose key runs it, or None to run it on no key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UseChoices {
    pub resume: Option<String>,
    pub application: Option<String>,
    pub job_evaluation: Option<String>,
}

/// Why a key couldn't be read or kept.
#[derive(Debug)]
pub enum Error {
    Database(sqlx::Error),
    Vault(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Database(e) => write!(f, "the database couldn't answer: {e}"),
            Error::Vault(e) => f.write_str(e),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Error::Database(e)
    }
}

/// The saved keys' details without the keys, oldest first.
pub async fn saved(db: &SqlitePool, user_id: i64) -> sqlx::Result<Vec<Saved>> {
    let rows = sqlx::query_file!("queries/account_model/saved.sql", user_id)
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| Saved {
            provider: row.provider,
            model: row.model,
            base_url: row.base_url,
            key_hint: row.key_hint,
            uses: Uses {
                resume: row.use_for_resume != 0,
                application: row.use_for_application != 0,
                job_evaluation: row.use_for_job_evaluation != 0,
            },
            updated_at: row.updated_at,
        })
        .collect())
}

/// The saved key the user picked for use_ (a name in USES), or None.
pub fn picked_for<'a>(saved: &'a [Saved], use_: &str) -> Option<&'a Saved> {
    saved.iter().find(|saved| saved.uses.of(use_))
}

/// The key to call the model with for use_, or None without a usable one: none picked for it,
/// or picked but no longer in Credential Manager.
pub async fn key_for(db: &SqlitePool, vault: &KeyVault, user_id: i64, use_: &str) -> Result<Option<KeyFor>, Error> {
    let _together = vault.lock().await;
    let saved = saved(db, user_id).await?;
    match picked_for(&saved, use_) {
        Some(picked) => with_key(vault, user_id, picked),
        None => Ok(None),
    }
}

/// The provider's saved key, or None when none is saved or it is no longer in Credential Manager.
pub async fn key_of(db: &SqlitePool, vault: &KeyVault, user_id: i64, provider: &str) -> Result<Option<KeyFor>, Error> {
    let _together = vault.lock().await;
    let saved = saved(db, user_id).await?;
    match saved.iter().find(|saved| saved.provider == provider) {
        Some(found) => with_key(vault, user_id, found),
        None => Ok(None),
    }
}

/// A saved key with its key from Credential Manager, or None when it isn't there. A local model
/// saved without a key runs with none. The caller holds the vault's lock.
pub fn with_key(vault: &KeyVault, user_id: i64, saved: &Saved) -> Result<Option<KeyFor>, Error> {
    let api_key = if saved.key_hint.is_empty() {
        Some(String::new())
    } else {
        kept_key(vault, user_id, saved)?
    };
    Ok(api_key.map(|api_key| KeyFor {
        provider: saved.provider.clone(),
        model: saved.model.clone(),
        base_url: saved.base_url.clone(),
        api_key,
        uses: saved.uses,
    }))
}

/// The saved key's key in Credential Manager. A key kept before keys were kept per provider is
/// moved to its provider's credential the first time it is read, when its last characters show
/// it is this row's.
fn kept_key(vault: &KeyVault, user_id: i64, saved: &Saved) -> Result<Option<String>, Error> {
    if let Some(key) = vault.key_for(user_id, &saved.provider).map_err(Error::Vault)? {
        return Ok(Some(key));
    }
    let before = vault.key_from_before_providers(user_id).map_err(Error::Vault)?;
    let Some(key) = before.filter(|key| key_hint(key) == saved.key_hint) else {
        return Ok(None);
    };
    vault.save(user_id, &saved.provider, &key).map_err(Error::Vault)?;
    vault.delete_key_from_before_providers(user_id).map_err(Error::Vault)?;
    Ok(Some(key))
}

/// Runs each use on the key chosen for it, and on no other.
pub async fn save_uses(db: &SqlitePool, user_id: i64, chosen: &UseChoices) -> sqlx::Result<()> {
    sqlx::query!(
        r#"
        UPDATE user_llm_keys
        SET use_for_resume = provider IS ?2,
            use_for_application = provider IS ?3,
            use_for_job_evaluation = provider IS ?4
        WHERE trongate_user_id = ?1
        "#,
        user_id,
        chosen.resume,
        chosen.application,
        chosen.job_evaluation
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Saves the provider's key, replacing the one saved for it before (its uses stay as they were);
/// other providers' keys stay. A new provider's key takes the uses no other key has. api_key ""
/// (a local model without one) keeps no key in Credential Manager.
///
/// The row and the credential change together, under the vault's lock, so no one reads one
/// without the other: the row is written in a transaction, a failed credential write leaves the
/// row as it was, and a failed commit puts the previous key back.
pub async fn save(
    db: &SqlitePool,
    vault: &KeyVault,
    user_id: i64,
    provider: &str,
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<(), Error> {
    let hint = key_hint(api_key);
    let now = chrono::Utc::now().timestamp();
    let _together = vault.lock().await;
    let mut tx = crate::db::write_transaction(db).await?;
    sqlx::query_file!(
        "queries/account_model/save.sql",
        user_id,
        provider,
        model,
        base_url,
        hint,
        now
    )
    .execute(&mut *tx)
    .await?;
    let previous = vault.key_for(user_id, provider).map_err(Error::Vault)?;
    keep(vault, user_id, provider, api_key)?;
    if let Err(e) = tx.commit().await {
        if let Err(restore) = keep(vault, user_id, provider, previous.as_deref().unwrap_or("")) {
            crate::log_line!("account: the previous key couldn't be put back after a failed save: {restore}");
        }
        return Err(e.into());
    }
    Ok(())
}

/// Keeps api_key as the user's key for provider in Credential Manager, or none when it is "".
fn keep(vault: &KeyVault, user_id: i64, provider: &str, api_key: &str) -> Result<(), Error> {
    let kept = if api_key.is_empty() {
        vault.delete(user_id, provider)
    } else {
        vault.save(user_id, provider, api_key)
    };
    kept.map_err(Error::Vault)
}

/// Deletes the provider's saved key, from the table and from Credential Manager. The uses it was
/// picked for run on no key until another is picked.
pub async fn delete_key(db: &SqlitePool, vault: &KeyVault, user_id: i64, provider: &str) -> Result<(), Error> {
    let _together = vault.lock().await;
    sqlx::query!(
        r#"DELETE FROM user_llm_keys WHERE trongate_user_id = ?1 AND provider = ?2"#,
        user_id,
        provider
    )
    .execute(db)
    .await?;
    vault.delete(user_id, provider).map_err(Error::Vault)
}

/// What the page calls a provider.
pub fn provider_label(provider: &str) -> &str {
    PROVIDERS
        .iter()
        .find(|(name, _)| *name == provider)
        .map(|(_, label)| *label)
        .unwrap_or(provider)
}

/// The key's last characters, which the page shows to tell keys apart.
fn key_hint(api_key: &str) -> String {
    let count = api_key.chars().count();
    api_key
        .chars()
        .skip(count.saturating_sub(KEY_HINT_CHARACTERS))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    const OPENAI_KEY: &str = "sk-proj-0123456789abcdWXYZ";
    const ANTHROPIC_KEY: &str = "sk-ant-api03-0123456789-abcd";

    async fn with_user() -> SqlitePool {
        let db = db::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO trongate_users (id) VALUES (1)")
            .execute(&db)
            .await
            .unwrap();
        db
    }

    fn all_uses() -> Uses {
        Uses {
            resume: true,
            application: true,
            job_evaluation: true,
        }
    }

    fn provider_for(saved: &[Saved], use_: &str) -> Option<String> {
        picked_for(saved, use_).map(|saved| saved.provider.clone())
    }

    #[tokio::test]
    async fn a_saved_key_is_in_the_vault_and_only_its_hint_in_the_table() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, OPENAI, "", "", OPENAI_KEY).await.unwrap();
        let found = saved(&db, 1).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key_hint, "WXYZ");
        assert_eq!(found[0].uses, all_uses(), "the first key takes every use");
        let key = key_for(&db, &vault, 1, RESUME).await.unwrap().unwrap();
        assert_eq!(key.api_key, OPENAI_KEY);
        assert!(!format!("{key:?}").contains("WXYZ"), "the key stays out of logs");

        delete_key(&db, &vault, 1, OPENAI).await.unwrap();
        assert_eq!(saved(&db, 1).await.unwrap(), []);
        assert_eq!(vault.key_for(1, OPENAI).unwrap(), None);
    }

    #[tokio::test]
    async fn a_second_providers_key_is_kept_beside_the_first() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, ANTHROPIC, "", "", ANTHROPIC_KEY).await.unwrap();
        save(&db, &vault, 1, OPENAI, "", "", OPENAI_KEY).await.unwrap();

        let found = saved(&db, 1).await.unwrap();
        let providers: Vec<&str> = found.iter().map(|saved| saved.provider.as_str()).collect();
        assert_eq!(providers, [ANTHROPIC, OPENAI]);
        assert_eq!(found[1].uses, Uses::default(), "the uses stay on the first key");
        let key = key_of(&db, &vault, 1, ANTHROPIC).await.unwrap().unwrap();
        assert_eq!(key.api_key, ANTHROPIC_KEY);
        let key = key_of(&db, &vault, 1, OPENAI).await.unwrap().unwrap();
        assert_eq!(key.api_key, OPENAI_KEY);
    }

    #[tokio::test]
    async fn each_use_runs_on_the_key_picked_for_it() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, ANTHROPIC, "claude-opus-5-5", "", ANTHROPIC_KEY)
            .await
            .unwrap();
        save(&db, &vault, 1, OPENAI, "", "", OPENAI_KEY).await.unwrap();
        let chosen = UseChoices {
            resume: Some(OPENAI.to_string()),
            application: Some(ANTHROPIC.to_string()),
            job_evaluation: None,
        };
        save_uses(&db, 1, &chosen).await.unwrap();

        let found = saved(&db, 1).await.unwrap();
        assert_eq!(provider_for(&found, RESUME).as_deref(), Some(OPENAI));
        assert_eq!(provider_for(&found, APPLICATION).as_deref(), Some(ANTHROPIC));
        assert_eq!(provider_for(&found, JOB_EVALUATION), None);
        let key = key_for(&db, &vault, 1, APPLICATION).await.unwrap().unwrap();
        assert_eq!(
            (key.provider.as_str(), key.model.as_str()),
            (ANTHROPIC, "claude-opus-5-5")
        );
        assert!(key_for(&db, &vault, 1, JOB_EVALUATION).await.unwrap().is_none());

        // Saved again, a key keeps its uses.
        save(&db, &vault, 1, ANTHROPIC, "", "", ANTHROPIC_KEY).await.unwrap();
        let found = saved(&db, 1).await.unwrap();
        assert_eq!(provider_for(&found, APPLICATION).as_deref(), Some(ANTHROPIC));
        assert_eq!(found[0].model, "");
    }

    #[tokio::test]
    async fn a_key_missing_from_the_vault_is_not_usable() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, OPENAI, "", "", OPENAI_KEY).await.unwrap();
        vault.delete(1, OPENAI).unwrap();
        assert_eq!(saved(&db, 1).await.unwrap().len(), 1);
        assert!(key_for(&db, &vault, 1, RESUME).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_key_kept_before_providers_moves_to_its_providers_credential() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, OPENAI, "", "", OPENAI_KEY).await.unwrap();
        vault.delete(1, OPENAI).unwrap();
        vault.save(1, "", OPENAI_KEY).unwrap();

        let key = key_for(&db, &vault, 1, RESUME).await.unwrap().unwrap();
        assert_eq!(key.api_key, OPENAI_KEY);
        assert_eq!(vault.key_for(1, OPENAI).unwrap().as_deref(), Some(OPENAI_KEY));
        assert_eq!(vault.key_from_before_providers(1).unwrap(), None);
    }

    #[tokio::test]
    async fn a_key_kept_before_providers_that_isnt_the_rows_stays_put() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, ANTHROPIC, "", "", ANTHROPIC_KEY).await.unwrap();
        vault.delete(1, ANTHROPIC).unwrap();
        vault.save(1, "", OPENAI_KEY).unwrap();

        assert!(key_of(&db, &vault, 1, ANTHROPIC).await.unwrap().is_none());
        assert_eq!(vault.key_from_before_providers(1).unwrap().as_deref(), Some(OPENAI_KEY));
    }

    #[tokio::test]
    async fn a_local_model_runs_without_a_key() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, LOCAL, "llama3.1:8b", "http://localhost:11434/v1", "")
            .await
            .unwrap();
        let key = key_for(&db, &vault, 1, JOB_EVALUATION).await.unwrap().unwrap();
        assert_eq!(
            (key.api_key.as_str(), key.base_url.as_str()),
            ("", "http://localhost:11434/v1")
        );
        assert_eq!(saved(&db, 1).await.unwrap()[0].key_hint, "");
    }
}
