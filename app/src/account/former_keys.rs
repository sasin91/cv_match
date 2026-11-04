//! The secrets the app kept in Windows Credential Manager (the system keyring elsewhere, through
//! the keyring crate) before the secrets table (key_vault): each provider's API key, the one key
//! from before keys were kept per provider, and the paired devices' token hashes. At start they
//! are moved into the table once, each read back from it before its credential is removed; the
//! cleanup removes any still left.
//!
//! A credential was named by the database's full path, so each database had its own set. Before
//! that, the names had the local user's trongate_users id in them too (before migration
//! 0010_one_local_user), and while the app was called Recruiter they were filed under its name.

use sqlx::SqlitePool;

use super::key_vault::{KeyVault, Secret};

/// The service the credentials were filed under.
const SERVICE: &str = "JobFinder";

/// The service they were filed under while the app was called Recruiter.
const FORMER_SERVICE: &str = "Recruiter";

/// Where one database's secrets may have been kept.
pub struct FormerKeys {
    /// The database's full path (db::full_path), which named its credentials.
    pub database: String,
    /// The local user's id while the database had trongate_users, which named them then.
    pub former_user_id: Option<i64>,
    /// Recruiter's database's full path, when Recruiter's keys are this database's to take.
    pub recruiter_database: Option<String>,
    /// The paired devices, whose token hashes had a credential each.
    pub device_ids: Vec<i64>,
}

/// One credential a secret may be in.
pub struct Credential {
    pub secret: Secret<'static>,
    service: &'static str,
    user: String,
}

impl Credential {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(self.service, &self.user).map_err(failed)
    }
}

impl FormerKeys {
    /// Where database's secrets may have been kept: former_user_id is the local user's id while
    /// the database had trongate_users (read before db::open's migrations drop it), and
    /// device_ids its paired devices. Recruiter's keys are looked for only when its database
    /// wasn't left in place beside this one (from_recruiter).
    pub fn of(database: &std::path::Path, former_user_id: Option<i64>, device_ids: Vec<i64>) -> Self {
        let recruiter_database = (!crate::from_recruiter::former_left_in_place(database))
            .then(|| crate::from_recruiter::former_key_database(database));
        FormerKeys {
            database: crate::db::full_path(database),
            former_user_id,
            recruiter_database,
            device_ids,
        }
    }

    /// Every credential to look in, the newest names first, so a key under a newer name is the
    /// one that is moved when there are two.
    pub fn credentials(&self) -> Vec<Credential> {
        let mut credentials = Vec::new();
        for (provider, secret) in llm_secrets() {
            credentials.push(Credential {
                secret,
                service: SERVICE,
                user: credential_user(&self.database, provider),
            });
        }
        for device_id in &self.device_ids {
            credentials.push(Credential {
                secret: Secret::DeviceTokenHash(*device_id),
                service: SERVICE,
                user: device_credential_user(&self.database, *device_id),
            });
        }
        let Some(user_id) = self.former_user_id else {
            return credentials;
        };
        for (provider, secret) in llm_secrets() {
            credentials.push(Credential {
                secret,
                service: SERVICE,
                user: credential_user_named_by_user(&self.database, user_id, provider),
            });
        }
        if let Some(recruiter) = &self.recruiter_database {
            for (provider, secret) in llm_secrets() {
                credentials.push(Credential {
                    secret,
                    service: FORMER_SERVICE,
                    user: credential_user_named_by_user(recruiter, user_id, provider),
                });
            }
        }
        credentials
    }

    /// Moves each secret the table lacks from its credential into vault (the database db's):
    /// kept, read back, and only then removed from the credential. How many were moved. The
    /// system's store isn't asked at all when the table lacks nothing, so a keyring that asks to be
    /// unlocked does so only while something is still to move. Err when it can't be asked (no
    /// keyring here).
    pub async fn move_into(&self, vault: &KeyVault, db: &SqlitePool) -> Result<usize, String> {
        let _together = vault.lock().await;
        let missing = missing_names(vault, db, &self.device_ids).await?;
        if missing.is_empty() {
            return Ok(0);
        }
        let mut moved = 0;
        let wanted = self
            .credentials()
            .into_iter()
            .filter(|credential| missing.contains(&credential.secret.name()));
        for credential in wanted {
            let entry = credential.entry()?;
            let Some(value) = password_of(&entry)? else {
                continue;
            };
            if vault.kept(db, &credential.secret).await?.is_none() {
                vault.keep(db, &credential.secret, &value).await?;
                if vault.kept(db, &credential.secret).await?.as_deref() != Some(value.as_str()) {
                    return Err(format!(
                        "{} didn't read back from the secrets table as it was saved, so it is left in place.",
                        credential.secret.name()
                    ));
                }
                moved += 1;
            }
            forget(&entry)?;
        }
        Ok(moved)
    }

    /// The credentials that still hold a secret, for the cleanup.
    pub fn left(&self) -> Result<Vec<Credential>, String> {
        let mut left = Vec::new();
        for credential in self.credentials() {
            if password_of(&credential.entry()?)?.is_some() {
                left.push(credential);
            }
        }
        Ok(left)
    }
}

/// The names of the secrets the table should have and lacks: the key of each saved key with one
/// (and then the key from before providers, which may be it), and each paired device's token
/// hash. A secret that is there but doesn't open isn't missing: it stays for the user to save
/// again, rather than be replaced by an older one.
async fn missing_names(vault: &KeyVault, db: &SqlitePool, device_ids: &[i64]) -> Result<Vec<String>, String> {
    let saved = super::account_model::saved(db)
        .await
        .map_err(|e| format!("The saved keys couldn't be read: {e}"))?;
    let mut wanted: Vec<Secret<'_>> = saved
        .iter()
        .filter(|saved| !saved.key_hint.is_empty())
        .map(|saved| Secret::LlmKey(&saved.provider))
        .collect();
    wanted.extend(device_ids.iter().map(|id| Secret::DeviceTokenHash(*id)));
    let mut missing = Vec::new();
    for secret in &wanted {
        if matches!(vault.kept(db, secret).await, Ok(None)) {
            missing.push(secret.name());
        }
    }
    if missing.iter().any(|name| name.starts_with("llm_key:")) {
        missing.push(Secret::KeyFromBeforeProviders.name());
    }
    Ok(missing)
}

/// Removes credential; none there is fine.
pub fn remove(credential: &Credential) -> Result<(), String> {
    forget(&credential.entry()?)
}

/// Each provider's key's secret, the key from before providers among them (as provider "").
fn llm_secrets() -> impl Iterator<Item = (&'static str, Secret<'static>)> {
    super::account_model::PROVIDERS
        .iter()
        .map(|(provider, _)| (*provider, Secret::LlmKey(provider)))
        .chain([(BEFORE_PROVIDERS, Secret::KeyFromBeforeProviders)])
}

/// The provider part of the credential the app kept its one key in before a key was kept per
/// provider (user_llm_keys@{database}).
const BEFORE_PROVIDERS: &str = "";

/// The value kept in credential, or None when there is none.
fn password_of(credential: &keyring::Entry) -> Result<Option<String>, String> {
    match credential.get_password() {
        Ok(value) => Ok(Some(value)),
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

/// devices:{id}@{database}: the credential a paired device's token hash was kept in.
fn device_credential_user(database: &str, device_id: i64) -> String {
    format!("devices:{device_id}@{database}")
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
    if cfg!(windows) {
        format!("Windows Credential Manager: {e}")
    } else {
        format!("The system keyring: {e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: &str = r"C:\Users\me\AppData\Roaming\JobFinder\jobfinder.sqlite";

    fn users(former: &FormerKeys) -> Vec<String> {
        former
            .credentials()
            .into_iter()
            .map(|credential| format!("{}/{}", credential.service, credential.user))
            .collect()
    }

    #[tokio::test]
    async fn only_secrets_the_table_lacks_are_looked_for() {
        let db = crate::db::open_in_memory().await.unwrap();
        let vault = KeyVault::with_key_in_memory();
        assert_eq!(
            missing_names(&vault, &db, &[]).await.unwrap(),
            Vec::<String>::new(),
            "nothing saved, so the keyring isn't asked"
        );
        super::super::account_model::save(&db, &vault, "openai", "", "", "sk-one")
            .await
            .unwrap();
        super::super::account_model::save(&db, &vault, "llama_cpp", "a.gguf", "", "")
            .await
            .unwrap();
        assert_eq!(missing_names(&vault, &db, &[]).await.unwrap(), Vec::<String>::new());
        vault.delete(&db, "openai").await.unwrap();
        assert_eq!(
            missing_names(&vault, &db, &[4]).await.unwrap(),
            ["llm_key:openai", "device_token_hash:4", "llm_key_from_before_providers"],
            "a local model without a key needs none"
        );
    }

    #[test]
    fn each_database_and_provider_had_its_own_credential() {
        assert_ne!(
            credential_user(MAIN, "openai"),
            credential_user(r"C:\Users\me\scratch.sqlite", "openai")
        );
        assert_ne!(credential_user(MAIN, "openai"), credential_user(MAIN, "anthropic"));
        assert_eq!(credential_user(MAIN, "openai"), format!("user_llm_keys:openai@{MAIN}"));
        assert_eq!(credential_user(MAIN, BEFORE_PROVIDERS), format!("user_llm_keys@{MAIN}"));
    }

    #[test]
    fn the_names_from_before_have_the_local_users_id() {
        assert_eq!(
            credential_user_named_by_user(MAIN, 3, "openai"),
            format!("user_llm_keys:3:openai@{MAIN}")
        );
        assert_eq!(
            credential_user_named_by_user(MAIN, 3, BEFORE_PROVIDERS),
            format!("user_llm_keys:3@{MAIN}"),
            "the credential the app kept its one key in before providers"
        );
    }

    #[test]
    fn a_database_without_a_former_user_looks_under_its_own_names_only() {
        let former = FormerKeys {
            database: MAIN.to_string(),
            former_user_id: None,
            recruiter_database: Some("old".to_string()),
            device_ids: vec![7],
        };
        let users = users(&former);
        assert!(users.contains(&format!("JobFinder/user_llm_keys:openai@{MAIN}")));
        assert!(users.contains(&format!("JobFinder/devices:7@{MAIN}")));
        assert!(users.iter().all(|user| user.starts_with("JobFinder/")), "{users:?}");
    }

    #[test]
    fn a_former_user_looks_under_recruiters_names_last() {
        let former = FormerKeys {
            database: MAIN.to_string(),
            former_user_id: Some(3),
            recruiter_database: Some("old.sqlite".to_string()),
            device_ids: Vec::new(),
        };
        let users = users(&former);
        let newest = users
            .iter()
            .position(|user| *user == format!("JobFinder/user_llm_keys:openai@{MAIN}"))
            .unwrap();
        let by_user = users
            .iter()
            .position(|user| *user == format!("JobFinder/user_llm_keys:3:openai@{MAIN}"))
            .unwrap();
        let recruiter = users
            .iter()
            .position(|user| user == "Recruiter/user_llm_keys:3:openai@old.sqlite")
            .unwrap();
        assert!(newest < by_user && by_user < recruiter, "{users:?}");
    }
}
