//! The user's own language model key (user_llm_keys), ported from
//! modules/account/Account_model.php, and what it is used for (use_for_*). The key itself is in
//! Windows Credential Manager (KeyVault); the table keeps its provider, model, last 4 characters
//! and, for a local model, base_url. The plain key only leaves here through key_for(), for the
//! llm module.

use std::fmt;

use sqlx::SqlitePool;

use super::key_vault::KeyVault;
use crate::cv_match::match_documents::Uses;
use crate::llm::{ANTHROPIC, LOCAL, OPENAI};

/// provider => what the page calls it.
pub const PROVIDERS: [(&str, &str); 3] = [
    (OPENAI, "OpenAI"),
    (ANTHROPIC, "Anthropic (Claude)"),
    (LOCAL, "A local model (Ollama, LM Studio)"),
];

/// What the key can be used for, each switched on or off by the user: purpose => what the page
/// calls it. A new key has them all.
pub const USES: [(&str, &str); 3] = [
    (RESUME, "Tailoring résumés"),
    (APPLICATION, "Writing job applications"),
    (JOB_EVALUATION, "Evaluating job posts (the AI match)"),
];
pub use crate::cv_match::document_box::{APPLICATION, RESUME};
pub const JOB_EVALUATION: &str = "job_evaluation";

/// How many of the key's last characters the page shows.
const KEY_HINT_CHARACTERS: usize = 4;

/// The saved key's details without the key.
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

/// Why the key couldn't be read or kept.
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

/// The saved key's details without the key, or None.
pub async fn saved(db: &SqlitePool, user_id: i64) -> sqlx::Result<Option<Saved>> {
    let row = sqlx::query_file!("queries/account_model/saved.sql", user_id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|row| Saved {
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
    }))
}

/// The key to call the model with, or None without a usable one: none saved, or saved but no
/// longer in Credential Manager. A local model saved without a key runs with none.
pub async fn key_for(db: &SqlitePool, vault: &KeyVault, user_id: i64) -> Result<Option<KeyFor>, Error> {
    let Some(saved) = saved(db, user_id).await? else {
        return Ok(None);
    };
    let api_key = if saved.key_hint.is_empty() {
        Some(String::new())
    } else {
        vault.key_for(user_id).map_err(Error::Vault)?
    };
    Ok(api_key.map(|api_key| KeyFor {
        provider: saved.provider,
        model: saved.model,
        base_url: saved.base_url,
        api_key,
        uses: saved.uses,
    }))
}

/// Switches the saved key's uses: those in on are on, the rest off.
pub async fn save_uses(db: &SqlitePool, user_id: i64, on: Uses) -> sqlx::Result<()> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query!(
        r#"
        UPDATE user_llm_keys
        SET use_for_resume = ?2, use_for_application = ?3, use_for_job_evaluation = ?4, updated_at = ?5
        WHERE trongate_user_id = ?1
        "#,
        user_id,
        on.resume,
        on.application,
        on.job_evaluation,
        now
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Saves the key, replacing any saved one (its uses stay as they were). api_key "" (a local
/// model without one) keeps no key in Credential Manager.
///
/// The row and the credential change together: the row is written in a transaction, which holds
/// SQLite's write lock while Credential Manager is changed, so two saves can't interleave; a
/// failed credential write leaves the row as it was, and a failed commit puts the previous key
/// back.
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
    let mut tx = db.begin().await?;
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
    let previous = vault.key_for(user_id).map_err(Error::Vault)?;
    keep(vault, user_id, api_key)?;
    if let Err(e) = tx.commit().await {
        if let Err(restore) = keep(vault, user_id, previous.as_deref().unwrap_or("")) {
            eprintln!("account: the previous key couldn't be put back after a failed save: {restore}");
        }
        return Err(e.into());
    }
    Ok(())
}

/// Keeps api_key as the user's in Credential Manager, or none when it is "".
fn keep(vault: &KeyVault, user_id: i64, api_key: &str) -> Result<(), Error> {
    let kept = if api_key.is_empty() {
        vault.delete(user_id)
    } else {
        vault.save(user_id, api_key)
    };
    kept.map_err(Error::Vault)
}

/// Deletes the saved key, from the table and from Credential Manager.
pub async fn delete_key(db: &SqlitePool, vault: &KeyVault, user_id: i64) -> Result<(), Error> {
    sqlx::query!(r#"DELETE FROM user_llm_keys WHERE trongate_user_id = ?1"#, user_id)
        .execute(db)
        .await?;
    vault.delete(user_id).map_err(Error::Vault)
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

    async fn with_user() -> SqlitePool {
        let db = db::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO trongate_users (id) VALUES (1)")
            .execute(&db)
            .await
            .unwrap();
        db
    }

    #[tokio::test]
    async fn a_saved_key_is_in_the_vault_and_only_its_hint_in_the_table() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, OPENAI, "", "", "sk-proj-0123456789abcdWXYZ")
            .await
            .unwrap();
        let found = saved(&db, 1).await.unwrap().unwrap();
        assert_eq!(found.key_hint, "WXYZ");
        assert_eq!(
            found.uses,
            Uses {
                resume: true,
                application: true,
                job_evaluation: true
            }
        );
        let key = key_for(&db, &vault, 1).await.unwrap().unwrap();
        assert_eq!(key.api_key, "sk-proj-0123456789abcdWXYZ");
        assert!(!format!("{key:?}").contains("WXYZ"), "the key stays out of logs");

        save_uses(
            &db,
            1,
            Uses {
                resume: false,
                application: true,
                job_evaluation: false,
            },
        )
        .await
        .unwrap();
        save(
            &db,
            &vault,
            1,
            ANTHROPIC,
            "claude-opus-5-5",
            "",
            "sk-ant-api03-0123456789-abcd",
        )
        .await
        .unwrap();
        let found = saved(&db, 1).await.unwrap().unwrap();
        assert_eq!(
            (found.provider.as_str(), found.model.as_str()),
            (ANTHROPIC, "claude-opus-5-5")
        );
        assert_eq!(
            found.uses,
            Uses {
                resume: false,
                application: true,
                job_evaluation: false
            }
        );

        delete_key(&db, &vault, 1).await.unwrap();
        assert_eq!(saved(&db, 1).await.unwrap(), None);
        assert_eq!(vault.key_for(1).unwrap(), None);
    }

    #[tokio::test]
    async fn a_key_missing_from_the_vault_is_not_usable() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, OPENAI, "", "", "sk-proj-0123456789abcdWXYZ")
            .await
            .unwrap();
        vault.delete(1).unwrap();
        assert!(saved(&db, 1).await.unwrap().is_some());
        assert!(key_for(&db, &vault, 1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_local_model_runs_without_a_key() {
        let db = with_user().await;
        let vault = KeyVault::in_memory();
        save(&db, &vault, 1, LOCAL, "llama3.1:8b", "http://localhost:11434/v1", "")
            .await
            .unwrap();
        let key = key_for(&db, &vault, 1).await.unwrap().unwrap();
        assert_eq!(
            (key.api_key.as_str(), key.base_url.as_str()),
            ("", "http://localhost:11434/v1")
        );
        assert_eq!(saved(&db, 1).await.unwrap().unwrap().key_hint, "");
    }
}
