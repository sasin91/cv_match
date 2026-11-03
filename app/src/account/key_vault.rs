//! Where the user's API keys are kept, not in the database: Windows Credential Manager (through
//! the keyring crate), so a key is never in the database file. Each provider's key has its own
//! credential, and each database file its own set, so neither a second key nor a second database
//! (--database) can replace another one's key. A paired device's token (devices) is kept the
//! same way, in a credential of its own. Tests keep them in memory instead.
//!
//! The app has one local user, so a credential is named by the database and provider alone.
//! Before, each name had the local user's trongate_users id in it too; take_over_keys_named_by_user
//! moves those once.
//!
//! A key and its row in llm_keys are two stores, so whoever reads or changes them takes the
//! vault's lock() first: a reader never pairs one key's row with another key.

use std::collections::HashMap;
use std::sync::Mutex;

/// The service the keys' credentials are filed under in Credential Manager.
const SERVICE: &str = "JobFinder";

/// The service the keys were filed under while the app was called Recruiter; see
/// take_over_former_keys().
const FORMER_SERVICE: &str = "Recruiter";

/// The provider part of the credential the app kept its one key in before a key was kept per
/// provider (user_llm_keys@{database}); see key_from_before_providers().
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
    /// Keys in this process only, for tests: provider => key.
    InMemory(Mutex<HashMap<String, String>>),
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
    pub fn save(&self, provider: &str, key: &str) -> Result<(), String> {
        self.save_secret(&Secret::LlmKey(provider), key)
    }

    /// The user's key for provider, or None when none is kept.
    pub fn key_for(&self, provider: &str) -> Result<Option<String>, String> {
        self.secret(&Secret::LlmKey(provider))
    }

    /// Forgets the user's key for provider; none kept is fine.
    pub fn delete(&self, provider: &str) -> Result<(), String> {
        self.forget_secret(&Secret::LlmKey(provider))
    }

    /// Keeps the token of the paired device with this id (devices), replacing one kept before.
    pub fn save_device_token(&self, device_id: i64, token: &str) -> Result<(), String> {
        self.save_secret(&Secret::DeviceToken(device_id), token)
    }

    /// The token of the paired device with this id, or None when none is kept.
    pub fn device_token(&self, device_id: i64) -> Result<Option<String>, String> {
        self.secret(&Secret::DeviceToken(device_id))
    }

    /// Forgets the token of the device with this id; none kept is fine.
    pub fn forget_device_token(&self, device_id: i64) -> Result<(), String> {
        self.forget_secret(&Secret::DeviceToken(device_id))
    }

    fn save_secret(&self, secret: &Secret, value: &str) -> Result<(), String> {
        match &self.store {
            Store::CredentialManager { database } => secret.entry(database)?.set_password(value).map_err(failed),
            Store::InMemory(kept) => {
                lock(kept).insert(secret.in_memory_name(), value.to_string());
                Ok(())
            }
        }
    }

    fn secret(&self, secret: &Secret) -> Result<Option<String>, String> {
        match &self.store {
            Store::CredentialManager { database } => password_of(&secret.entry(database)?),
            Store::InMemory(kept) => Ok(lock(kept).get(&secret.in_memory_name()).cloned()),
        }
    }

    fn forget_secret(&self, secret: &Secret) -> Result<(), String> {
        match &self.store {
            Store::CredentialManager { database } => forget(&secret.entry(database)?),
            Store::InMemory(kept) => {
                lock(kept).remove(&secret.in_memory_name());
                Ok(())
            }
        }
    }

    /// The one key the app kept for the user before it kept a key per provider, or None. It
    /// belongs to whichever provider's row was saved then; the caller moves it there.
    pub fn key_from_before_providers(&self) -> Result<Option<String>, String> {
        self.key_for(BEFORE_PROVIDERS)
    }

    /// Forgets the key from before providers, once it is moved.
    pub fn delete_key_from_before_providers(&self) -> Result<(), String> {
        self.delete(BEFORE_PROVIDERS)
    }

    /// Moves the keys that the app kept while it was called Recruiter (under FORMER_SERVICE, for
    /// the database then at former_database's full path, named by the local user's id then) to
    /// this vault. How many were moved; none kept there is fine.
    pub fn take_over_former_keys(&self, former_database: &str, former_user_id: i64) -> Result<usize, String> {
        self.take_over(|provider| {
            entry_named(
                FORMER_SERVICE,
                &credential_user_named_by_user(former_database, former_user_id, provider),
            )
        })
    }

    /// Moves the keys this database's credentials kept while their names had the local user's
    /// trongate_users id in them (before migration 0010_one_local_user) to the names without it.
    /// How many were moved; none kept there is fine.
    pub fn take_over_keys_named_by_user(&self, former_user_id: i64) -> Result<usize, String> {
        let Store::CredentialManager { database } = &self.store else {
            return Ok(0);
        };
        self.take_over(|provider| {
            entry_named(
                SERVICE,
                &credential_user_named_by_user(database, former_user_id, provider),
            )
        })
    }

    /// Moves each provider's key from the credential former names for it to this vault: kept
    /// here unless a key for its provider already is, and then forgotten there. How many were
    /// moved.
    fn take_over(&self, former: impl Fn(&str) -> Result<keyring::Entry, String>) -> Result<usize, String> {
        if let Store::InMemory(_) = self.store {
            return Ok(0);
        }
        let mut moved = 0;
        for provider in every_provider() {
            let former = former(provider)?;
            let Some(key) = password_of(&former)? else {
                continue;
            };
            if self.key_for(provider)?.is_none() {
                self.save(provider, &key)?;
                moved += 1;
            }
            forget(&former)?;
        }
        Ok(moved)
    }

    /// The providers a key is kept for, the key from before providers among them (as ""), for
    /// the cleanup.
    pub fn kept(&self) -> Result<Vec<&'static str>, String> {
        let mut kept = Vec::new();
        for provider in every_provider() {
            if self.key_for(provider)?.is_some() {
                kept.push(provider);
            }
        }
        Ok(kept)
    }
}

/// What the vault keeps: a provider's API key, or a paired device's token.
enum Secret<'a> {
    LlmKey(&'a str),
    DeviceToken(i64),
}

impl Secret<'_> {
    /// Its credential in Credential Manager, for the database at this full path.
    fn entry(&self, database: &str) -> Result<keyring::Entry, String> {
        match self {
            Secret::LlmKey(provider) => entry(database, provider),
            Secret::DeviceToken(device_id) => entry_named(SERVICE, &device_credential_user(database, *device_id)),
        }
    }

    /// Its name among the in-memory store's: the provider, or device:{id}.
    fn in_memory_name(&self) -> String {
        match self {
            Secret::LlmKey(provider) => provider.to_string(),
            Secret::DeviceToken(device_id) => format!("device:{device_id}"),
        }
    }
}

/// devices:{id}@{database}: the credential a paired device's token is kept in.
fn device_credential_user(database: &str, device_id: i64) -> String {
    format!("devices:{device_id}@{database}")
}

/// Every provider a key can be kept for, the key from before providers among them (as "").
fn every_provider() -> impl Iterator<Item = &'static str> {
    super::account_model::PROVIDERS
        .iter()
        .map(|(provider, _)| *provider)
        .chain([BEFORE_PROVIDERS])
}

/// The credential for provider's key in Credential Manager.
fn entry(database: &str, provider: &str) -> Result<keyring::Entry, String> {
    entry_named(SERVICE, &credential_user(database, provider))
}

/// The credential named user, filed under service.
fn entry_named(service: &str, user: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, user).map_err(failed)
}

/// The key kept in credential, or None when there is none.
fn password_of(credential: &keyring::Entry) -> Result<Option<String>, String> {
    match credential.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(failed(e)),
    }
}

/// Forgets credential; none kept is fine.
fn forget(credential: &keyring::Entry) -> Result<(), String> {
    match credential.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(failed(e)),
    }
}

/// user_llm_keys:{provider}@{database}, or user_llm_keys@{database} for the key from before
/// providers.
fn credential_user(database: &str, provider: &str) -> String {
    if provider == BEFORE_PROVIDERS {
        format!("user_llm_keys@{database}")
    } else {
        format!("user_llm_keys:{provider}@{database}")
    }
}

/// The name a credential had while it was named by the local user's id too:
/// user_llm_keys:{id}:{provider}@{database}, or user_llm_keys:{id}@{database} for the key from
/// before providers.
fn credential_user_named_by_user(database: &str, user_id: i64, provider: &str) -> String {
    if provider == BEFORE_PROVIDERS {
        format!("user_llm_keys:{user_id}@{database}")
    } else {
        format!("user_llm_keys:{user_id}:{provider}@{database}")
    }
}

fn failed(e: keyring::Error) -> String {
    format!("Windows Credential Manager: {e}")
}

fn lock(keys: &Mutex<HashMap<String, String>>) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
    keys.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_key_is_read_back_until_deleted() {
        let vault = KeyVault::in_memory();
        assert_eq!(vault.key_for("openai").unwrap(), None);
        vault.save("openai", "sk-one").unwrap();
        vault.save("openai", "sk-two").unwrap();
        assert_eq!(vault.key_for("openai").unwrap().as_deref(), Some("sk-two"));
        vault.delete("openai").unwrap();
        vault.delete("openai").unwrap();
        assert_eq!(vault.key_for("openai").unwrap(), None);
    }

    #[test]
    fn each_provider_keeps_its_own_key() {
        let vault = KeyVault::in_memory();
        vault.save("anthropic", "sk-ant-one").unwrap();
        vault.save("openai", "sk-two").unwrap();
        assert_eq!(vault.key_for("anthropic").unwrap().as_deref(), Some("sk-ant-one"));
        assert_eq!(vault.key_for("openai").unwrap().as_deref(), Some("sk-two"));
    }

    #[test]
    fn each_database_and_provider_has_its_own_credential() {
        let main = r"C:\Users\me\AppData\Roaming\JobFinder\jobfinder.sqlite";
        assert_ne!(
            credential_user(main, "openai"),
            credential_user(r"C:\Users\me\scratch.sqlite", "openai")
        );
        assert_ne!(credential_user(main, "openai"), credential_user(main, "anthropic"));
        assert_eq!(credential_user(main, "openai"), format!("user_llm_keys:openai@{main}"));
        assert_eq!(credential_user(main, BEFORE_PROVIDERS), format!("user_llm_keys@{main}"));
    }

    #[test]
    fn the_names_from_before_have_the_local_users_id() {
        let main = r"C:\Users\me\AppData\Roaming\JobFinder\jobfinder.sqlite";
        assert_eq!(
            credential_user_named_by_user(main, 3, "openai"),
            format!("user_llm_keys:3:openai@{main}")
        );
        assert_eq!(
            credential_user_named_by_user(main, 3, BEFORE_PROVIDERS),
            format!("user_llm_keys:3@{main}"),
            "the credential the app kept its one key in before providers"
        );
    }
}
