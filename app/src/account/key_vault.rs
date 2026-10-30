//! Where the user's API key is kept, in place of modules/account/Key_vault.php's encryption with
//! the server's secret: Windows Credential Manager (through the keyring crate), so the key is
//! never in the database file. Tests keep keys in memory instead.

use std::collections::HashMap;
use std::sync::Mutex;

/// The service the key's credential is filed under in Credential Manager.
const SERVICE: &str = "Recruiter";

pub enum KeyVault {
    /// Windows Credential Manager (the system's secret store on other platforms).
    CredentialManager,
    /// Keys in this process only, for tests: user id => key.
    InMemory(Mutex<HashMap<i64, String>>),
}

impl KeyVault {
    pub fn in_memory() -> Self {
        KeyVault::InMemory(Mutex::new(HashMap::new()))
    }

    /// Keeps key as the user's, replacing any kept before.
    pub fn save(&self, user_id: i64, key: &str) -> Result<(), String> {
        match self {
            KeyVault::CredentialManager => entry(user_id)?.set_password(key).map_err(failed),
            KeyVault::InMemory(keys) => {
                lock(keys).insert(user_id, key.to_string());
                Ok(())
            }
        }
    }

    /// The user's key, or None when none is kept.
    pub fn key_for(&self, user_id: i64) -> Result<Option<String>, String> {
        match self {
            KeyVault::CredentialManager => match entry(user_id)?.get_password() {
                Ok(key) => Ok(Some(key)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(failed(e)),
            },
            KeyVault::InMemory(keys) => Ok(lock(keys).get(&user_id).cloned()),
        }
    }

    /// Forgets the user's key; none kept is fine.
    pub fn delete(&self, user_id: i64) -> Result<(), String> {
        match self {
            KeyVault::CredentialManager => match entry(user_id)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(failed(e)),
            },
            KeyVault::InMemory(keys) => {
                lock(keys).remove(&user_id);
                Ok(())
            }
        }
    }
}

/// The user's credential in Credential Manager: Recruiter, user_llm_keys:{id}.
fn entry(user_id: i64) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, &format!("user_llm_keys:{user_id}")).map_err(failed)
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
}
