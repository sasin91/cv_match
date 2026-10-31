//! Where the user's API key is kept, not in the database: Windows Credential Manager (through the
//! keyring crate), so the key is never in the database file. Each database file has its own
//! credential, so a second database (--database) can't replace the first one's key. Tests keep keys
//! in memory instead.
//!
//! The key and its row in user_llm_keys are two stores, so whoever reads or changes them takes
//! the vault's lock() first: a reader never pairs one key's row with another key.

use std::collections::HashMap;
use std::sync::Mutex;

/// The service the key's credential is filed under in Credential Manager.
const SERVICE: &str = "Recruiter";

pub struct KeyVault {
    store: Store,
    /// Held while a key and its row are read or changed together.
    together: tokio::sync::Mutex<()>,
}

enum Store {
    /// Windows Credential Manager (the system's secret store on other platforms), for the
    /// database at this full path.
    CredentialManager { database: String },
    /// Keys in this process only, for tests: user id => key.
    InMemory(Mutex<HashMap<i64, String>>),
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

    /// Waits until no one else is reading or changing the key and its row, and keeps them from
    /// it until the guard is dropped.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.together.lock().await
    }

    /// Keeps key as the user's, replacing any kept before.
    pub fn save(&self, user_id: i64, key: &str) -> Result<(), String> {
        match &self.store {
            Store::CredentialManager { database } => entry(database, user_id)?.set_password(key).map_err(failed),
            Store::InMemory(keys) => {
                lock(keys).insert(user_id, key.to_string());
                Ok(())
            }
        }
    }

    /// The user's key, or None when none is kept.
    pub fn key_for(&self, user_id: i64) -> Result<Option<String>, String> {
        match &self.store {
            Store::CredentialManager { database } => match entry(database, user_id)?.get_password() {
                Ok(key) => Ok(Some(key)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(failed(e)),
            },
            Store::InMemory(keys) => Ok(lock(keys).get(&user_id).cloned()),
        }
    }

    /// Forgets the user's key; none kept is fine.
    pub fn delete(&self, user_id: i64) -> Result<(), String> {
        match &self.store {
            Store::CredentialManager { database } => match entry(database, user_id)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(failed(e)),
            },
            Store::InMemory(keys) => {
                lock(keys).remove(&user_id);
                Ok(())
            }
        }
    }
}

/// The user's credential in Credential Manager: Recruiter, user_llm_keys:{id}@{database}.
fn entry(database: &str, user_id: i64) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &credential_user(database, user_id)).map_err(failed)
}

fn credential_user(database: &str, user_id: i64) -> String {
    format!("user_llm_keys:{user_id}@{database}")
}

fn failed(e: keyring::Error) -> String {
    format!("Windows Credential Manager: {e}")
}

fn lock(keys: &Mutex<HashMap<i64, String>>) -> std::sync::MutexGuard<'_, HashMap<i64, String>> {
    keys.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_key_is_read_back_until_deleted() {
        let vault = KeyVault::in_memory();
        assert_eq!(vault.key_for(1).unwrap(), None);
        vault.save(1, "sk-one").unwrap();
        vault.save(1, "sk-two").unwrap();
        assert_eq!(vault.key_for(1).unwrap().as_deref(), Some("sk-two"));
        assert_eq!(vault.key_for(2).unwrap(), None);
        vault.delete(1).unwrap();
        vault.delete(1).unwrap();
        assert_eq!(vault.key_for(1).unwrap(), None);
    }

    #[test]
    fn each_database_has_its_own_credential() {
        assert_ne!(
            credential_user(r"C:\Users\me\AppData\Roaming\Recruiter\recruiter.sqlite", 1),
            credential_user(r"C:\Users\me\scratch.sqlite", 1)
        );
    }
}
