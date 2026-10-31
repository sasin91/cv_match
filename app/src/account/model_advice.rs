//! Which model the app suggests to run its AI on: its own llama.cpp model first, when this
//! computer has room for a good one (llama_setup_service::good_local_model), else an OpenAI key,
//! or a Claude key. The API key page and the setup wizard both show it.

use super::llama_setup_service::{self, CatalogRow};
use crate::llama::llama_server::LlamaServer;
use crate::llm::{self, OPENAI};

/// The suggestion, for the pages.
pub struct ModelAdvice {
    /// The model to run on this computer, when it has room for a good one.
    pub local: Option<CatalogRow>,
    /// "31.1 GB memory, AMD Radeon RX 7600 with 7.9 GB", or "" without a llama folder.
    pub computer: String,
    /// The model an OpenAI key runs on by default, for the API key suggestion.
    pub openai_model: &'static str,
}

impl ModelAdvice {
    pub fn from(server: Option<&LlamaServer>) -> Self {
        ModelAdvice {
            local: server.and_then(llama_setup_service::good_local_model),
            computer: server.map(|server| server.inventory().to_string()).unwrap_or_default(),
            openai_model: llm::default_model(OPENAI).unwrap_or(""),
        }
    }

    /// Whether this app's own model is the suggestion.
    pub fn local_first(&self) -> bool {
        self.local.is_some()
    }
}
