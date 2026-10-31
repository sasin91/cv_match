//! The setup wizard for this app's own model (GET llama_setup): it downloads the llama.cpp build
//! for this computer's graphics card into the llama folder, offers the catalog's models with how
//! each fits, downloads the one picked into the models folder, and saves it as the "llama_cpp"
//! key. A download runs in the background; the page refreshes itself while it does. The work is
//! in llama_setup_service; the handlers here read the form, call it and print the page.

use std::sync::Arc;

use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Form, Json};
use serde::{Deserialize, Serialize};

use super::llama_setup_service::{self, CatalogRow, DownloadView, LlamaSetupView, ModelSaved, POLL_MILLISECONDS};
use crate::server::AppState;
use crate::setup;

/// Where the wizard is.
const PAGE: &str = "/llama_setup";

/// Where a saved model goes on to once the setup wizard is finished.
const ACCOUNT_PAGE: &str = "/account";

#[derive(Template)]
#[template(path = "account/llama_setup.html")]
struct LlamaSetupPage {
    flash: Option<String>,
    /// How often the progress bar asks llama_setup/progress, in milliseconds.
    poll_milliseconds: u32,
    computer: String,
    folder: String,
    models_folder: String,
    server_found: bool,
    /// The llama-server that runs: the llama folder's, or one on PATH.
    server_path: String,
    /// Which llama.cpp build this computer gets.
    build_label: &'static str,
    card_problem: Option<String>,
    download: Option<DownloadView>,
    /// Whether a download is running, so the buttons wait.
    busy: bool,
    /// The model to take, when one fits this computer.
    recommended: Option<CatalogRow>,
    /// The models that fit and aren't downloaded, for the download list (the recommended one
    /// selected).
    to_download: Vec<CatalogRow>,
    /// The downloaded models, for the list of the one to use (the recommended one selected).
    downloaded: Vec<CatalogRow>,
    /// The setup wizard isn't finished, so the page links back to it.
    setup_unfinished: bool,
}

impl LlamaSetupPage {
    fn new(view: LlamaSetupView, flash: Option<String>) -> Self {
        let LlamaSetupView {
            computer,
            folder,
            models_folder,
            server_found,
            server_path,
            build_label,
            card_problem,
            download,
            busy,
            recommended,
            to_download,
            downloaded,
            setup_unfinished,
        } = view;
        LlamaSetupPage {
            flash,
            poll_milliseconds: POLL_MILLISECONDS,
            computer,
            folder,
            models_folder,
            server_found,
            server_path,
            build_label,
            card_problem,
            download,
            busy,
            recommended,
            to_download,
            downloaded,
            setup_unfinished,
        }
    }
}

/// The model forms' field.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ModelForm {
    file: String,
}

/// GET llama_setup.
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    let view = match llama_setup_service::llama_setup_view(&state.core).await {
        Ok(view) => view,
        // Logged when it was made.
        Err(e) => return failure(e.message),
    };
    match LlamaSetupPage::new(view, state.take_flash()).render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            eprintln!("llama_setup: the page couldn't be printed: {e}");
            failure(format!("The page couldn't be printed: {e}"))
        }
    }
}

/// POST llama_setup/download_server: downloads llama.cpp's build for this card in the background.
pub async fn download_server(State(state): State<Arc<AppState>>) -> Response {
    if let Err(e) = llama_setup_service::download_server(&state.core) {
        state.set_flash(e.message);
    }
    Redirect::to(PAGE).into_response()
}

/// POST llama_setup/download_model {file}: downloads a catalog model in the background.
pub async fn download_model(State(state): State<Arc<AppState>>, Form(form): Form<ModelForm>) -> Response {
    if let Err(e) = llama_setup_service::download_model(&state.core, &form.file) {
        state.set_flash(e.message);
    }
    Redirect::to(PAGE).into_response()
}

/// POST llama_setup/use_model {file}: saves the downloaded model as this app's own model.
pub async fn use_model(State(state): State<Arc<AppState>>, Form(form): Form<ModelForm>) -> Response {
    match llama_setup_service::use_model(&state.core, &form.file).await {
        Ok(ModelSaved {
            message,
            setup_unfinished,
        }) => {
            state.set_flash(message);
            Redirect::to(if setup_unfinished { setup::PAGE } else { ACCOUNT_PAGE }).into_response()
        }
        Err(e) => {
            state.set_flash(e.message);
            Redirect::to(PAGE).into_response()
        }
    }
}

/// GET llama_setup/progress's answer, for the progress bar.
#[derive(Serialize)]
struct ProgressAnswer {
    running: bool,
    percent: Option<u64>,
    progress: String,
}

/// GET llama_setup/progress: how the download is going. The page reloads when it isn't running.
pub async fn progress(State(state): State<Arc<AppState>>) -> Response {
    let answer = match llama_setup_service::progress(&state.core) {
        Some(view) => ProgressAnswer {
            running: view.running,
            percent: view.percent,
            progress: view.progress,
        },
        None => ProgressAnswer {
            running: false,
            percent: None,
            progress: String::new(),
        },
    };
    Json(answer).into_response()
}

/// The page when it can't be shown: what went wrong.
fn failure(message: String) -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llama::hardware::{GIB, Gpu, GpuVendor, Inventory};
    use crate::llama::llama_server::LlamaServer;

    #[test]
    fn the_wizard_prints_for_an_8_gb_card() {
        let inventory = Inventory {
            memory_bytes: 32 * GIB,
            gpu: Some(Gpu {
                name: "AMD Radeon RX 7600".to_string(),
                vendor: GpuVendor::Amd,
                vram_bytes: 8 * GIB,
            }),
        };
        let folder = std::env::temp_dir().join("recruiter-llama-setup-page-test");
        let server = LlamaServer::new(&folder, inventory);
        let page = LlamaSetupPage::new(llama_setup_service::view_of(&server), None);
        let html = page.render().unwrap();
        assert!(html.contains("Qwen3-8B-Q4_K_M.gguf"));
    }
}
