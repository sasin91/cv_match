//! Where the app's secrets are kept: the user's API keys and the paired devices' token hashes, in
//! the database's secrets table, each sealed under the database's data key (data_key), which is
//! kept in a file beside the database and protected by the system. The same on every system, and
//! one database's secrets are its own: a second database (--database) has its own table and key.
//!
//! Before the secrets table they were in Windows Credential Manager (the keyring elsewhere);
//! former_keys moves them here once.
//!
//! A key and its row in llm_keys are read and changed together, so whoever does takes the vault's
//! lock() first: a reader never pairs one key's row with another key.

use sqlx::SqliteExecutor;

use super::data_key::DataKey;

pub struct KeyVault {
    /// The database's data key, or why it couldn't be read; each secret's read or write says so.
    data_key: Result<DataKey, String>,
    /// Held while a key and its row are read or changed together.
    together: tokio::sync::Mutex<()>,
}

/// What the vault keeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Secret<'a> {
    /// A provider's API key.
    LlmKey(&'a str),
    /// The one key the app kept before it kept a key per provider; see key_from_before_providers().
    KeyFromBeforeProviders,
    /// The hash of the token of the paired device with this id (devices). The token itself is
    /// never kept on this computer.
    DeviceTokenHash(i64),
}

impl Secret<'_> {
    /// Its name in the secrets table, which is sealed into its value too.
    pub fn name(&self) -> String {
        match self {
            Secret::LlmKey(provider) => format!("llm_key:{provider}"),
            Secret::KeyFromBeforeProviders => "llm_key_from_before_providers".to_string(),
            Secret::DeviceTokenHash(device_id) => format!("device_token_hash:{device_id}"),
        }
    }
}

impl KeyVault {
    /// The vault of the database at this path, its data key in the file beside it (made there
    /// when there is none yet).
    pub fn of_database(database: &std::path::Path) -> Self {
        let data_key = DataKey::of_database(database);
        if let Err(why) = &data_key {
            crate::log_line!("secrets: {why}");
        }
        Self::with(data_key)
    }

    /// A vault whose data key is kept nowhere: for a database that lives in memory (tests).
    pub fn with_key_in_memory() -> Self {
        Self::with(DataKey::random())
    }

    fn with(data_key: Result<DataKey, String>) -> Self {
        KeyVault {
            data_key,
            together: tokio::sync::Mutex::new(()),
        }
    }

    /// Waits until no one else is reading or changing a key and its row, and keeps them from it
    /// until the guard is dropped.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.together.lock().await
    }

    /// Keeps key as the user's key for provider, replacing the one kept for it before.
    pub async fn save<'c>(&self, db: impl SqliteExecutor<'c>, provider: &str, key: &str) -> Result<(), String> {
        self.keep(db, &Secret::LlmKey(provider), key).await
    }

    /// The user's key for provider, or None when none is kept.
    pub async fn key_for<'c>(&self, db: impl SqliteExecutor<'c>, provider: &str) -> Result<Option<String>, String> {
        self.kept(db, &Secret::LlmKey(provider)).await
    }

    /// Forgets the user's key for provider; none kept is fine.
    pub async fn delete<'c>(&self, db: impl SqliteExecutor<'c>, provider: &str) -> Result<(), String> {
        self.forget(db, &Secret::LlmKey(provider)).await
    }

    /// Keeps the hash of the token of the paired device with this id, replacing one kept before.
    pub async fn save_device_token_hash<'c>(
        &self,
        db: impl SqliteExecutor<'c>,
        device_id: i64,
        hash: &str,
    ) -> Result<(), String> {
        self.keep(db, &Secret::DeviceTokenHash(device_id), hash).await
    }

    /// The hash of the token of the paired device with this id, or None when none is kept.
    pub async fn device_token_hash<'c>(
        &self,
        db: impl SqliteExecutor<'c>,
        device_id: i64,
    ) -> Result<Option<String>, String> {
        self.kept(db, &Secret::DeviceTokenHash(device_id)).await
    }

    /// Forgets the token hash of the device with this id; none kept is fine.
    pub async fn forget_device_token_hash<'c>(
        &self,
        db: impl SqliteExecutor<'c>,
        device_id: i64,
    ) -> Result<(), String> {
        self.forget(db, &Secret::DeviceTokenHash(device_id)).await
    }

    /// The one key the app kept for the user before it kept a key per provider, or None. It
    /// belongs to whichever provider's row was saved then; the caller moves it there.
    pub async fn key_from_before_providers<'c>(&self, db: impl SqliteExecutor<'c>) -> Result<Option<String>, String> {
        self.kept(db, &Secret::KeyFromBeforeProviders).await
    }

    /// Forgets the key from before providers, once it is moved.
    pub async fn delete_key_from_before_providers<'c>(&self, db: impl SqliteExecutor<'c>) -> Result<(), String> {
        self.forget(db, &Secret::KeyFromBeforeProviders).await
    }

    /// Keeps value as secret, sealed, replacing the one kept before.
    pub async fn keep<'c>(&self, db: impl SqliteExecutor<'c>, secret: &Secret<'_>, value: &str) -> Result<(), String> {
        let name = secret.name();
        let sealed = self.data_key()?.seal(&name, value)?;
        let now = chrono::Utc::now().timestamp();
        sqlx::query!(
            r#"
            INSERT INTO secrets (name, sealed, updated_at)
            VALUES (?1, ?2, ?3)
            ON CONFLICT (name) DO UPDATE SET sealed = excluded.sealed, updated_at = excluded.updated_at
            "#,
            name,
            sealed,
            now
        )
        .execute(db)
        .await
        .map(|_| ())
        .map_err(|e| failed(&name, "kept", e))
    }

    /// secret's value, or None when none is kept.
    pub async fn kept<'c>(&self, db: impl SqliteExecutor<'c>, secret: &Secret<'_>) -> Result<Option<String>, String> {
        let name = secret.name();
        let sealed = sqlx::query_scalar!(r#"SELECT sealed FROM secrets WHERE name = ?1"#, name)
            .fetch_optional(db)
            .await
            .map_err(|e| failed(&name, "read", e))?;
        match sealed {
            Some(sealed) => self.data_key()?.open(&name, &sealed).map(Some),
            None => Ok(None),
        }
    }

    /// Forgets secret; none kept is fine.
    pub async fn forget<'c>(&self, db: impl SqliteExecutor<'c>, secret: &Secret<'_>) -> Result<(), String> {
        let name = secret.name();
        sqlx::query!(r#"DELETE FROM secrets WHERE name = ?1"#, name)
            .execute(db)
            .await
            .map(|_| ())
            .map_err(|e| failed(&name, "removed", e))
    }

    fn data_key(&self) -> Result<&DataKey, String> {
        self.data_key.as_ref().map_err(Clone::clone)
    }
}

fn failed(name: &str, done: &str, e: sqlx::Error) -> String {
    format!("The secret {name} couldn't be {done}: the database said {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> sqlx::SqlitePool {
        crate::db::open_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn a_kept_key_is_read_back_until_deleted() {
        let db = db().await;
        let vault = KeyVault::with_key_in_memory();
        assert_eq!(vault.key_for(&db, "openai").await.unwrap(), None);
        vault.save(&db, "openai", "sk-one").await.unwrap();
        vault.save(&db, "openai", "sk-two").await.unwrap();
        assert_eq!(vault.key_for(&db, "openai").await.unwrap().as_deref(), Some("sk-two"));
        vault.delete(&db, "openai").await.unwrap();
        vault.delete(&db, "openai").await.unwrap();
        assert_eq!(vault.key_for(&db, "openai").await.unwrap(), None);
    }

    #[tokio::test]
    async fn each_provider_keeps_its_own_key() {
        let db = db().await;
        let vault = KeyVault::with_key_in_memory();
        vault.save(&db, "anthropic", "sk-ant-one").await.unwrap();
        vault.save(&db, "openai", "sk-two").await.unwrap();
        assert_eq!(
            vault.key_for(&db, "anthropic").await.unwrap().as_deref(),
            Some("sk-ant-one")
        );
        assert_eq!(vault.key_for(&db, "openai").await.unwrap().as_deref(), Some("sk-two"));
    }

    #[tokio::test]
    async fn the_table_holds_no_key_in_plain_text() {
        let db = db().await;
        let vault = KeyVault::with_key_in_memory();
        vault.save(&db, "openai", "sk-plain-to-find").await.unwrap();
        let sealed: Vec<u8> = sqlx::query_scalar("SELECT sealed FROM secrets")
            .fetch_one(&db)
            .await
            .unwrap();
        assert!(!sealed.windows(5).any(|window| window == b"plain"));
    }

    #[tokio::test]
    async fn another_databases_key_cant_open_them() {
        let db = db().await;
        KeyVault::with_key_in_memory()
            .save(&db, "openai", "sk-one")
            .await
            .unwrap();
        let why = KeyVault::with_key_in_memory().key_for(&db, "openai").await.unwrap_err();
        assert!(why.contains("another key file"), "{why}");
    }

    #[tokio::test]
    async fn a_vault_without_its_key_says_why() {
        let db = db().await;
        let vault = KeyVault::with(Err("The key file x couldn't be read: denied".to_string()));
        let why = vault.save(&db, "openai", "sk-one").await.unwrap_err();
        assert!(why.contains("denied"), "{why}");
    }

    #[test]
    fn each_secret_has_its_own_name() {
        assert_eq!(Secret::LlmKey("openai").name(), "llm_key:openai");
        assert_eq!(Secret::DeviceTokenHash(3).name(), "device_token_hash:3");
        assert_ne!(Secret::KeyFromBeforeProviders.name(), Secret::LlmKey("").name());
    }
}
