//! The saved key as the API key page shows it (the saved-key box): its provider's name, last
//! characters, model and address, the day it was saved, and a box per use.

use chrono::{Local, TimeZone};

use super::account_model::{self, Saved, USES};

/// A use's box: its form field (use_for_{name}), what the page calls it, and whether it is on.
pub struct KeyUse {
    pub field: String,
    pub label: &'static str,
    pub on: bool,
}

pub struct SavedKey {
    pub provider: String,
    pub key_hint: String,
    pub model: String,
    pub base_url: String,
    /// The day it was saved, as "30 Oct 2026".
    pub saved_on: String,
    pub uses: Vec<KeyUse>,
}

impl SavedKey {
    pub fn from(saved: &Saved) -> Self {
        SavedKey {
            provider: account_model::provider_label(&saved.provider).to_string(),
            key_hint: saved.key_hint.clone(),
            model: saved.model.clone(),
            base_url: saved.base_url.clone(),
            saved_on: day_of(saved.updated_at),
            uses: USES
                .iter()
                .map(|&(name, label)| KeyUse {
                    field: format!("use_for_{name}"),
                    label,
                    on: saved.uses.of(name),
                })
                .collect(),
        }
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
