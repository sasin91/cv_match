//! The setup wizard for this app's own model (GET llama_setup): it downloads the llama.cpp build
//! for this computer's graphics card into the llama folder, offers the catalog's models with how
//! each fits (model_choice, penny's weighing), downloads the one picked into the models folder,
//! and saves it as the "llama_cpp" key. A download runs in the background; the page refreshes
//! itself while it does.

use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::account_model;
use crate::llama::catalog::{self, CatalogModel};
use crate::llama::download::{self, Build, Progress};
use crate::llama::hardware;
use crate::llama::llama_server::LlamaServer;
use crate::llama::model_choice::{self, Fit};
use crate::llm::LLAMA_CPP;
use crate::server::AppState;

/// Where the wizard is.
const PAGE: &str = "/llama_setup";

/// What the download of llama.cpp is called while it runs.
const SERVER_DOWNLOAD: &str = "llama.cpp";

/// How often the page reloads itself while a download runs, in seconds.
const REFRESH_SECONDS: u32 = 2;

/// Percent, for the download's progress.
const PERCENT: u64 = 100;

/// A catalog model as the wizard lists it.
pub struct CatalogRow {
    pub file: &'static str,
    pub label: &'static str,
    /// Its size, as "4.7 GB".
    pub size: String,
    /// How it would fit, as model_choice::Fit::label says.
    pub fit: String,
    /// Too big for this computer: it can't be downloaded here.
    pub too_big: bool,
    /// The biggest of the best fit: the one to take.
    pub recommended: bool,
    /// In the models folder already.
    pub downloaded: bool,
}

/// The running or last download.
pub struct DownloadView {
    pub what: String,
    /// "1.2 GB of 5.0 GB (24%)", or "" when it isn't running.
    pub progress: String,
    pub running: bool,
    pub finished: bool,
    pub why_failed: Option<String>,
}

#[derive(Template)]
#[template(path = "account/llama_setup.html")]
struct LlamaSetupPage {
    flash: Option<String>,
    /// Seconds before the page reloads itself, while a download runs.
    refresh_seconds: Option<u32>,
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
    models: Vec<CatalogRow>,
}

/// The model forms' field.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ModelForm {
    file: String,
}

/// GET llama_setup.
pub async fn index(State(state): State<Arc<AppState>>) -> Response {
    let Some(server) = &state.llama else {
        return failure("The app was started without its llama folder.");
    };
    let page = page_of(server, state.take_flash());
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => failure(&format!("The page couldn't be printed: {e}")),
    }
}

/// POST llama_setup/download_server: downloads llama.cpp's build for this card in the background.
pub async fn download_server(State(state): State<Arc<AppState>>) -> Response {
    start_download(&state, SERVER_DOWNLOAD, |state| async move {
        let Some(server) = &state.llama else {
            return Err("The app was started without its llama folder.".to_string());
        };
        download::download_server(server.folder(), server.inventory(), &server.downloads).await
    })
}

/// POST llama_setup/download_model {file}: downloads a catalog model in the background.
pub async fn download_model(State(state): State<Arc<AppState>>, Form(form): Form<ModelForm>) -> Response {
    let Some(model) = catalog::find(&form.file) else {
        state.set_flash(format!("{} isn't one of the models offered.", form.file));
        return Redirect::to(PAGE).into_response();
    };
    start_download(&state, model.file, move |state| async move {
        let Some(server) = &state.llama else {
            return Err("The app was started without its llama folder.".to_string());
        };
        download::download_model(model, &server.models_folder(), &server.downloads).await
    })
}

/// POST llama_setup/use_model {file}: saves the downloaded model as this app's own model.
pub async fn use_model(State(state): State<Arc<AppState>>, Form(form): Form<ModelForm>) -> Response {
    let Some(server) = &state.llama else {
        return failure("The app was started without its llama folder.");
    };
    let downloaded = server.models().iter().any(|model| model.name == form.file);
    if !downloaded {
        state.set_flash(format!(
            "{} isn't in the models folder yet. Download it first.",
            form.file
        ));
        return Redirect::to(PAGE).into_response();
    }
    let saved = account_model::save(
        state.db(),
        &state.key_vault,
        state.user_id,
        LLAMA_CPP,
        &form.file,
        "",
        "",
    )
    .await;
    match saved {
        Ok(()) => {
            state.set_flash(format!(
                "This app's own model runs on {}. Pick below what it is used for, and press Check to load it.",
                form.file
            ));
            Redirect::to("/account").into_response()
        }
        Err(e) => {
            eprintln!("llama_setup: the model couldn't be saved: {e}");
            state.set_flash(format!("The model couldn't be saved: {e}"));
            Redirect::to(PAGE).into_response()
        }
    }
}

/// Starts work as the one download, called what, unless one is running; back to the page either way.
fn start_download<Work, Done>(state: &Arc<AppState>, what: &str, work: Work) -> Response
where
    Work: FnOnce(Arc<AppState>) -> Done + Send + 'static,
    Done: Future<Output = Result<(), String>> + Send,
{
    let Some(server) = &state.llama else {
        return failure("The app was started without its llama folder.");
    };
    if let Err(why) = server.downloads.begin(what) {
        state.set_flash(why);
        return Redirect::to(PAGE).into_response();
    }
    let state = Arc::clone(state);
    tokio::spawn(async move {
        let outcome = work(Arc::clone(&state)).await;
        if let Some(server) = &state.llama {
            server.downloads.end(outcome);
        }
    });
    Redirect::to(PAGE).into_response()
}

fn page_of(server: &LlamaServer, flash: Option<String>) -> LlamaSetupPage {
    let overview = server.overview();
    let card_usable = server.card_usable().unwrap_or(false);
    let inventory = server.inventory();
    let models_folder = server.models_folder();
    let download = download_view(&server.downloads.progress());
    let busy = download.as_ref().is_some_and(|download| download.running);
    LlamaSetupPage {
        flash,
        refresh_seconds: busy.then_some(REFRESH_SECONDS),
        computer: overview.inventory.to_string(),
        folder: overview.folder.display().to_string(),
        models_folder: models_folder.display().to_string(),
        server_found: overview.server.is_some(),
        server_path: overview
            .server
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        build_label: Build::for_computer(inventory).label(),
        card_problem: overview.card_problem,
        download,
        busy,
        models: catalog_rows(server, card_usable),
    }
}

/// The catalog, each model weighed against this computer, with the one to take marked.
fn catalog_rows(server: &LlamaServer, card_usable: bool) -> Vec<CatalogRow> {
    let models_folder = server.models_folder();
    let inventory = server.inventory();
    let as_files: Vec<_> = catalog::MODELS
        .iter()
        .map(|model| model.as_gguf(&models_folder))
        .collect();
    let recommended = model_choice::choose(&as_files, "", inventory, card_usable)
        .map(|(model, _)| model.name.clone())
        .ok();
    let downloaded: Vec<String> = server.models().into_iter().map(|model| model.name).collect();
    catalog::MODELS
        .iter()
        .zip(&as_files)
        .map(|(model, file): (&CatalogModel, _)| {
            let fit = model_choice::fit(file, inventory, card_usable);
            CatalogRow {
                file: model.file,
                label: model.label,
                size: hardware::gigabytes(model.bytes),
                fit: fit.label(),
                too_big: fit == Fit::TooBig,
                recommended: recommended.as_deref() == Some(model.file),
                downloaded: downloaded.iter().any(|name| name == model.file),
            }
        })
        .collect()
}

fn download_view(progress: &Progress) -> Option<DownloadView> {
    let view = |what: &str| DownloadView {
        what: what.to_string(),
        progress: String::new(),
        running: false,
        finished: false,
        why_failed: None,
    };
    match progress {
        Progress::Idle => None,
        Progress::Running {
            what,
            done_bytes,
            total_bytes,
        } => Some(DownloadView {
            progress: progress_text(*done_bytes, *total_bytes),
            running: true,
            ..view(what)
        }),
        Progress::Finished { what } => Some(DownloadView {
            finished: true,
            ..view(what)
        }),
        Progress::Failed { what, why } => Some(DownloadView {
            why_failed: Some(why.clone()),
            ..view(what)
        }),
    }
}

/// "1.2 GB of 5.0 GB (24%)", or "1.2 GB" when the size isn't known.
fn progress_text(done_bytes: u64, total_bytes: Option<u64>) -> String {
    match total_bytes.filter(|total| *total > 0) {
        Some(total) => format!(
            "{} of {} ({}%)",
            hardware::gigabytes(done_bytes),
            hardware::gigabytes(total),
            done_bytes * PERCENT / total
        ),
        None => hardware::gigabytes(done_bytes),
    }
}

/// The page when it can't be shown: what went wrong, logged too.
fn failure(message: &str) -> Response {
    eprintln!("llama_setup: {message}");
    (StatusCode::INTERNAL_SERVER_ERROR, message.to_string()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llama::hardware::{GIB, Gpu, GpuVendor, Inventory};

    #[test]
    fn the_progress_says_how_much_of_how_much() {
        assert_eq!(progress_text(GIB, Some(4 * GIB)), "1.0 GB of 4.0 GB (25%)");
        assert_eq!(progress_text(GIB, None), "1.0 GB");
    }

    #[test]
    fn an_8_gb_card_is_offered_qwen3_8b() {
        // Jonas's PC: 31.1 GB memory, AMD Radeon RX 7600 with 7.9 GB.
        let inventory = Inventory {
            memory_bytes: 311 * GIB / 10,
            gpu: Some(Gpu {
                name: "AMD Radeon RX 7600".to_string(),
                vendor: GpuVendor::Amd,
                vram_bytes: 79 * GIB / 10,
            }),
        };
        let folder = std::env::temp_dir().join("recruiter-llama-setup-test");
        let server = LlamaServer::new(&folder, inventory);
        let page = page_of(&server, None);
        let recommended: Vec<&str> = page
            .models
            .iter()
            .filter(|row| row.recommended)
            .map(|row| row.file)
            .collect();
        assert_eq!(recommended, ["Qwen3-8B-Q4_K_M.gguf"]);
        assert_eq!(page.build_label, Build::Vulkan.label());
        assert_eq!(page.card_problem, None);
        assert!(page.render().is_ok());
    }
}
