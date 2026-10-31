//! Where the user's API keys are kept, not in the database: Windows Credential Manager (through
//! the keyring crate), so a key is never in the database file. Each provider's key has its own
//! credential, and each database file its own set, so neither a second key nor a second database
//! (--database) can replace another one's key. Tests keep keys in memory instead.
//!
//! A key and its row in user_llm_keys are two stores, so whoever reads or changes them takes the
//! vault's lock() first: a reader never pairs one key's row with another key.

use std::collections::HashMap;
use std::sync::Mutex;

/// The service the keys' credentials are filed under in Credential Manager.
const SERVICE: &str = "Recruiter";

/// The provider part of the credential the app kept its one key in before a key was kept per
/// provider (user_llm_keys:{id}@{database}); see key_from_before_providers().
const BEFORE_PROVIDERS: &str = "";

pub struct KeyVault {
    store: Store,
    /// Held while a key and its row are read or changed together.
    together: tokio::sync::Mutex<()>,
}

enum Store {
    /// Windows Credential Manager (the system's secret store on other platforms), for the
    /// database at this full path.
    CredentialManager { database: String },
    /// Keys in this process only, for tests: (user id, provider) => key.
    InMemory(Mutex<HashMap<(i64, String), String>>),
}

impl KeyVault {
    /// Windows Credential Manager, for the database at this full path.
    pub fn credential_manager(database: String) -> Self {
        Self::with(Store::CredentialManager { database })
    }

    pub fn in_memory() -> Self {
        Self::with(Store::InMemory(Mutex::new(HashMap::new())))
    }

    fn with(store: Store) -> Self {
        KeyVault {
            store,
            together: tokio::sync::Mutex::new(()),
        }
    }

    /// Waits until no one else is reading or changing a key and its row, and keeps them from it
    /// until the guard is dropped.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.together.lock().await
    }

    /// Keeps key as the user's key for provider, replacing the one kept for it before.
    pub fn save(&self, user_id: i64, provider: &str, key: &str) -> Result<(), String> {
        match &self.store {
            Store::CredentialManager { database } => {
                entry(database, user_id, provider)?.set_password(key).map_err(failed)
            }
            Store::InMemory(keys) => {
                lock(keys).insert((user_id, provider.to_string()), key.to_string());
                Ok(())
            }
        }
    }

    /// The user's key for provider, or None when none is kept.
    pub fn key_for(&self, user_id: i64, provider: &str) -> Result<Option<String>, String> {
        match &self.store {
            Store::CredentialManager { database } => match entry(database, user_id, provider)?.get_password() {
                Ok(key) => Ok(Some(key)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(failed(e)),
            },
            Store::InMemory(keys) => Ok(lock(keys).get(&(user_id, provider.to_string())).cloned()),
        }
    }

    /// Forgets the user's key for provider; none kept is fine.
    pub fn delete(&self, user_id: i64, provider: &str) -> Result<(), String> {
        match &self.store {
            Store::CredentialManager { database } => match entry(database, user_id, provider)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(failed(e)),
            },
            Store::InMemory(keys) => {
                lock(keys).remove(&(user_id, provider.to_string()));
                Ok(())
            }
        }
    }

    /// The one key the app kept for the user before it kept a key per provider, or None. It
    /// belongs to whichever provider's row was saved then; the caller moves it there.
    pub fn key_from_before_providers(&self, user_id: i64) -> Result<Option<String>, String> {
        self.key_for(user_id, BEFORE_PROVIDERS)
    }

    /// Forgets the key from before providers, once it is moved.
    pub fn delete_key_from_before_providers(&self, user_id: i64) -> Result<(), String> {
        self.delete(user_id, BEFORE_PROVIDERS)
    }
}

/// The user's credential for provider in Credential Manager.
fn entry(database: &str, user_id: i64, provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &credential_user(database, user_id, provider)).map_err(failed)
}

/// user_llm_keys:{id}:{provider}@{database}, or user_llm_keys:{id}@{database} for the key from
/// before providers.
fn credential_user(database: &str, user_id: i64, provider: &str) -> String {
    if provider == BEFORE_PROVIDERS {
        format!("user_llm_keys:{user_id}@{database}")
    } else {
        format!("user_llm_keys:{user_id}:{provider}@{database}")
    }
}

fn failed(e: keyring::Error) -> String {
    format!("Windows Credential Manager: {e}")
}

fn lock(keys: &Mutex<HashMap<(i64, String), String>>) -> std::sync::MutexGuard<'_, HashMap<(i64, String), String>> {
    keys.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_key_is_read_back_until_deleted() {
        let vault = KeyVault::in_memory();
        assert_eq!(vault.key_for(1, "openai").unwrap(), None);
        vault.save(1, "openai", "sk-one").unwrap();
        vault.save(1, "openai", "sk-two").unwrap();
        assert_eq!(vault.key_for(1, "openai").unwrap().as_deref(), Some("sk-two"));
        assert_eq!(vault.key_for(2, "openai").unwrap(), None);
        vault.delete(1, "openai").unwrap();
        vault.delete(1, "openai").unwrap();
        assert_eq!(vault.key_for(1, "openai").unwrap(), None);
    }

    #[test]
    fn each_provider_keeps_its_own_key() {
        let vault = KeyVault::in_memory();
        vault.save(1, "anthropic", "sk-ant-one").unwrap();
        vault.save(1, "openai", "sk-two").unwrap();
        assert_eq!(vault.key_for(1, "anthropic").unwrap().as_deref(), Some("sk-ant-one"));
        assert_eq!(vault.key_for(1, "openai").unwrap().as_deref(), Some("sk-two"));
    }

    #[test]
    fn each_database_and_provider_has_its_own_credential() {
        let main = r"C:\Users\me\AppData\Roaming\Recruiter\recruiter.sqlite";
        assert_ne!(
            credential_user(main, 1, "openai"),
            credential_user(r"C:\Users\me\scratch.sqlite", 1, "openai")
        );
        assert_ne!(
            credential_user(main, 1, "openai"),
            credential_user(main, 1, "anthropic")
        );
        assert_eq!(
            credential_user(main, 1, BEFORE_PROVIDERS),
            format!("user_llm_keys:1@{main}"),
            "the credential the app kept its one key in before"
        );
    }
}
