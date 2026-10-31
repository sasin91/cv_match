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
use crate::setup;

/// Where the wizard is.
const PAGE: &str = "/llama_setup";

/// What the download of llama.cpp is called while it runs.
const SERVER_DOWNLOAD: &str = "llama.cpp";

/// How often the page reloads itself while a download runs, in seconds.
const REFRESH_SECONDS: u32 = 2;

/// Percent, for the download's progress and the card's share of a model.
const PERCENT: u64 = 100;

/// The share of a model's layers that must be on the graphics card for it to be suggested as
/// this app's own model, in percent: with less, most of it runs on the CPU, which is slow.
const MIN_CARD_SHARE_PERCENT: u64 = 50;

/// A catalog model as the wizard lists it.
#[derive(Clone)]
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
    /// Good enough for the AI match (catalog::is_good_enough) and whole or partly on the
    /// graphics card, so it answers in reasonable time.
    pub good_enough: bool,
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
    let mut page = page_of(server, state.take_flash());
    page.setup_unfinished = setup::first_run_or_logged(&state).await;
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
    let first_run = setup::first_run_or_logged(&state).await;
    match saved {
        Ok(()) if first_run => {
            state.set_flash(format!(
                "This app's own model runs on {}. It starts on the first AI call, which reads your CV below.",
                form.file
            ));
            Redirect::to(setup::PAGE).into_response()
        }
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
    let card_usable = card_usable_for_weighing(server);
    let inventory = server.inventory();
    let models_folder = server.models_folder();
    let download = download_view(&server.downloads.progress());
    let busy = download.as_ref().is_some_and(|download| download.running);
    let ModelLists {
        recommended,
        to_download,
        downloaded,
    } = model_lists(catalog_rows(server, card_usable));
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
        recommended,
        to_download,
        downloaded,
        setup_unfinished: false,
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
                good_enough: catalog::is_good_enough(model) && runs_mostly_on_the_card(fit, model.layers),
            }
        })
        .collect()
}

/// Whether a model of layers with this fit has at least MIN_CARD_SHARE_PERCENT of them on the
/// graphics card, so it answers in reasonable time.
fn runs_mostly_on_the_card(fit: Fit, layers: u32) -> bool {
    match fit {
        Fit::Gpu => true,
        Fit::Split { gpu_layers } => u64::from(gpu_layers) * PERCENT >= u64::from(layers) * MIN_CARD_SHARE_PERCENT,
        Fit::Memory | Fit::TooBig => false,
    }
}

/// The model to suggest as this app's own: the recommended one when it is good enough, else the
/// smallest good-enough one, which has the most of itself on the card; None when this computer has no room for a good model, so an OpenAI
/// or Claude key is the better suggestion.
pub fn good_local_model(server: &LlamaServer) -> Option<CatalogRow> {
    let rows = catalog_rows(server, card_usable_for_weighing(server));
    let recommended = rows.iter().find(|row| row.recommended && row.good_enough);
    recommended.or_else(|| rows.iter().find(|row| row.good_enough)).cloned()
}

/// Whether the card counts when models are weighed: as llama_server says, and also when the
/// llama-server there lacks the card's compute library, since the wizard downloads the build
/// that has it.
fn card_usable_for_weighing(server: &LlamaServer) -> bool {
    server.card_usable().unwrap_or(true)
}

/// The catalog as the page lists it: the recommended model, the ones still to download, and the
/// downloaded ones. A model too big for this computer is in neither list.
struct ModelLists {
    recommended: Option<CatalogRow>,
    to_download: Vec<CatalogRow>,
    downloaded: Vec<CatalogRow>,
}

fn model_lists(rows: Vec<CatalogRow>) -> ModelLists {
    let recommended = rows.iter().find(|row| row.recommended).cloned();
    let (downloaded, to_download): (Vec<CatalogRow>, Vec<CatalogRow>) = rows
        .into_iter()
        .filter(|row| !row.too_big)
        .partition(|row| row.downloaded);
    ModelLists {
        recommended,
        to_download,
        downloaded,
    }
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
        assert_eq!(
            page.recommended.as_ref().map(|row| row.file),
            Some("Qwen3-8B-Q4_K_M.gguf")
        );
        let offered: Vec<&str> = page.to_download.iter().map(|row| row.file).collect();
        assert_eq!(
            offered.len(),
            4,
            "every model fits a 7.9 GB card one way or another: {offered:?}"
        );
        assert!(page.downloaded.is_empty());
        assert_eq!(page.build_label, Build::Vulkan.label());
        assert_eq!(page.card_problem, None);
        assert!(page.render().is_ok());
    }

    #[test]
    fn a_computer_without_room_on_its_card_for_a_good_model_gets_none() {
        let folder = std::env::temp_dir().join("recruiter-llama-advice-test");
        let card = |vram_tenths: u64| Inventory {
            memory_bytes: 16 * GIB,
            gpu: Some(Gpu {
                name: "A card".to_string(),
                vendor: GpuVendor::Nvidia,
                vram_bytes: vram_tenths * GIB / 10,
            }),
        };
        let jonas = LlamaServer::new(&folder, card(79));
        assert_eq!(
            good_local_model(&jonas).map(|row| row.file),
            Some("Qwen3-8B-Q4_K_M.gguf")
        );
        // Only the 1.7B fits whole on a 4 GB card; under half of the 4B fits on it.
        let small_card = LlamaServer::new(&folder, card(40));
        assert_eq!(good_local_model(&small_card).map(|row| row.file), None);
        // On 5 GB most of the 4B is on the card, though the 1.7B is the recommended fit.
        let five_gb = LlamaServer::new(&folder, card(50));
        assert_eq!(
            good_local_model(&five_gb).map(|row| row.file),
            Some("Qwen3-4B-Q4_K_M.gguf")
        );
        let no_card = LlamaServer::new(
            &folder,
            Inventory {
                memory_bytes: 16 * GIB,
                gpu: None,
            },
        );
        assert_eq!(good_local_model(&no_card).map(|row| row.file), None);
    }
}
