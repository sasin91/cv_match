//! The saved keys as the API key page shows them: per key (the saved-key boxes) its provider's
//! name, last characters, model and address and the day it was saved; and per use the list that
//! picks the key it runs on.

use chrono::{Local, TimeZone};

use super::account_model::{self, Saved, USES};

/// What the use lists offer to run a use on no key.
const NO_KEY: &str = "No key (switched off)";

pub struct SavedKey {
    /// The provider as the forms send it.
    pub provider_value: String,
    /// What the page calls the provider.
    pub provider: String,
    pub key_hint: String,
    pub model: String,
    pub base_url: String,
    /// The day it was saved, as "30 Oct 2026".
    pub saved_on: String,
    /// Saved, but no longer in Credential Manager.
    pub unreadable: bool,
}

impl SavedKey {
    pub fn from(saved: &Saved, unreadable: bool) -> Self {
        SavedKey {
            provider_value: saved.provider.clone(),
            provider: account_model::provider_label(&saved.provider).to_string(),
            key_hint: saved.key_hint.clone(),
            model: saved.model.clone(),
            base_url: saved.base_url.clone(),
            saved_on: day_of(saved.updated_at),
            unreadable,
        }
    }
}

/// An entry in a use's list: a saved key's provider, or "" for no key.
pub struct KeyChoice {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

/// A use's list: its form field (use_for_{name}), what the page calls it, and the keys to pick.
pub struct KeyUse {
    pub field: String,
    pub label: &'static str,
    pub choices: Vec<KeyChoice>,
}

/// A list per use, each offering every saved key and no key, with the picked one selected.
pub fn key_uses(saved: &[Saved]) -> Vec<KeyUse> {
    USES.iter()
        .map(|&(name, label)| KeyUse {
            field: format!("use_for_{name}"),
            label,
            choices: choices_for(saved, name),
        })
        .collect()
}

fn choices_for(saved: &[Saved], use_: &str) -> Vec<KeyChoice> {
    let picked = account_model::picked_for(saved, use_).map(|saved| saved.provider.as_str());
    let keys = saved.iter().map(|saved| KeyChoice {
        value: saved.provider.clone(),
        label: key_label(saved),
        selected: picked == Some(saved.provider.as_str()),
    });
    let none = KeyChoice {
        value: String::new(),
        label: NO_KEY.to_string(),
        selected: picked.is_none(),
    };
    keys.chain(std::iter::once(none)).collect()
}

/// "OpenAI (…65sA)", or the provider's name alone for a key without last characters.
fn key_label(saved: &Saved) -> String {
    let provider = account_model::provider_label(&saved.provider);
    if saved.key_hint.is_empty() {
        provider.to_string()
    } else {
        format!("{provider} (…{})", saved.key_hint)
    }
}

/// A Unix time as "21 Oct 2026", in this computer's time zone.
fn day_of(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.format("%-d %b %Y").to_string())
        .unwrap_or_default()
}
