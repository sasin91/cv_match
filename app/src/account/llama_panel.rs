//! The API key page's box for this app's own model: the llama folder, whether llama-server is in
//! it, the computer the models are weighed against, and each model with how it fits.

use crate::llama::hardware;
use crate::llama::llama_server::{self, Overview};

/// A model in the models folder.
pub struct ModelRow {
    pub name: String,
    /// Its size, as "4.7 GB".
    pub size: String,
    /// How it fits, as model_choice::Fit::label says.
    pub fit: String,
    /// Whether a blank model field runs it.
    pub chosen: bool,
}

pub struct LlamaPanel {
    pub folder: String,
    pub server_file: &'static str,
    pub server_found: bool,
    pub models_folder: String,
    /// "32.0 GB memory, NVIDIA GeForce RTX 4070 with 12.0 GB".
    pub computer: String,
    pub card_problem: Option<String>,
    pub models: Vec<ModelRow>,
    /// Why no model can run with a blank model field, when none can.
    pub no_choice: Option<String>,
}

impl LlamaPanel {
    pub fn from(overview: Overview) -> Self {
        let chosen = overview.chosen.as_ref().ok();
        LlamaPanel {
            folder: overview.folder.display().to_string(),
            server_file: llama_server::SERVER_FILE,
            server_found: overview.server_found,
            models_folder: overview.folder.join(llama_server::MODELS_FOLDER).display().to_string(),
            computer: overview.inventory.to_string(),
            card_problem: overview.card_problem.clone(),
            models: overview
                .models
                .iter()
                .map(|(model, fit)| ModelRow {
                    name: model.name.clone(),
                    size: hardware::gigabytes(model.bytes),
                    fit: fit.label(),
                    chosen: chosen == Some(&model.name),
                })
                .collect(),
            no_choice: overview.chosen.err(),
        }
    }
}
