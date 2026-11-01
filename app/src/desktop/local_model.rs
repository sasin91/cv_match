//! The Local model screen: the llama.cpp setup page (llama_setup) in the window. This computer,
//! llama-server, the download running with its progress, and the models to download or use,
//! all through llama_setup_service, the functions the web page uses.

use std::sync::Arc;
use std::time::Duration;

use iced::widget::{Column, button, column, container, progress_bar, row, text};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, CARD_PADDING, ROW_PADDING};
use super::{GAP, Runner};
use crate::account::llama_setup_service::{self, CatalogRow, LlamaSetupView};
use crate::dashboard::Health;

/// How often the screen reads a running download's progress.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);
/// The progress bar's height.
const BAR_HEIGHT: f32 = 6.0;
/// Percent, the bar's whole.
const PERCENT: f32 = 100.0;

#[derive(Default)]
pub struct State {
    /// None until read; Err says why it couldn't be.
    view: Option<Result<Arc<LlamaSetupView>, String>>,
    /// What the last click came to.
    said: Option<(Health, String)>,
    /// A reload is on its way, so a download's progress isn't asked for twice.
    polling: bool,
}

#[derive(Clone)]
pub enum Msg {
    Load,
    Loaded(Result<Arc<LlamaSetupView>, String>),
    DownloadServer,
    DownloadModel(&'static str),
    UseModel(&'static str),
    /// What a click came to; a reload follows.
    Said(Result<String, String>),
    /// The next look at a running download.
    Poll,
    /// A downloaded model is saved as this app's own model; the shell goes on to where its uses
    /// are picked (Language models), or to Setup while that is unfinished, saying message there.
    ModelInUse {
        message: String,
        setup_unfinished: bool,
    },
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            return runner.run(
                |context| async move { llama_setup_service::llama_setup_view(&context).await.map(Arc::new) },
                |read| Msg::Loaded(read.and_then(|view| view.map_err(|e| e.message))),
            );
        }
        Msg::Loaded(view) => {
            let running = view.as_ref().is_ok_and(|view| view.busy);
            state.view = Some(view);
            if running && !state.polling {
                state.polling = true;
                return runner.run(|_| tokio::time::sleep(PROGRESS_EVERY), |_| Msg::Poll);
            }
        }
        Msg::Poll => {
            state.polling = false;
            return Task::done(Msg::Load);
        }
        Msg::DownloadServer => {
            return runner.run(
                |context| async move {
                    llama_setup_service::download_server(&context).map(|()| "Downloading llama.cpp…".to_string())
                },
                said,
            );
        }
        Msg::DownloadModel(file) => {
            return runner.run(
                move |context| async move {
                    llama_setup_service::download_model(&context, file).map(|()| format!("Downloading {file}…"))
                },
                said,
            );
        }
        Msg::UseModel(file) => {
            return runner.run(
                move |context| async move { llama_setup_service::use_model(&context, file).await },
                |answer| match answer.and_then(|saved| saved.map_err(|e| e.message)) {
                    Ok(saved) => Msg::ModelInUse {
                        message: saved.message,
                        setup_unfinished: saved.setup_unfinished,
                    },
                    Err(why) => Msg::Said(Err(why)),
                },
            );
        }
        // The shell goes on.
        Msg::ModelInUse { .. } => {}
        Msg::Said(said) => {
            state.said = Some(match said {
                Ok(said) => (Health::Ready, said),
                Err(why) => (Health::Attention, why),
            });
            return Task::done(Msg::Load);
        }
    }
    Task::none()
}

/// A service's answer as Msg::Said.
fn said(answer: Result<Result<String, crate::service_error::ServiceError>, String>) -> Msg {
    Msg::Said(answer.and_then(|said| said.map_err(|e| e.message)))
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Local model",
        widgets::note(
            "The app runs a model itself with llama.cpp: no API key, nothing paid per use, and your CV stays here.",
        ),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    let view = match &state.view {
        None => return screen.push(widgets::note("Looking at this computer…")).into(),
        Some(Err(why)) => {
            return screen
                .push(widgets::message_bar(Health::Attention, why.as_str()))
                .into();
        }
        Some(Ok(view)) => view,
    };
    screen = screen.push(widgets::section("This computer", computer(view)));
    if let Some(download) = &view.download {
        screen = screen.push(widgets::section("Download", download_card(download)));
    }
    if !view.downloaded.is_empty() {
        screen = screen.push(widgets::section(
            "Downloaded models",
            widgets::rows(
                view.downloaded
                    .iter()
                    .map(|model| model_row(model, ModelAction::Use, view.busy)),
            ),
        ));
    }
    if !view.to_download.is_empty() {
        screen = screen.push(widgets::section(
            "Models to download",
            widgets::rows(
                view.to_download
                    .iter()
                    .map(|model| model_row(model, ModelAction::Download, view.busy)),
            ),
        ));
    }
    screen
        .push(widgets::caption(format!("Models are kept in {}.", view.models_folder)))
        .into()
}

/// The computer the models are weighed against, and llama-server.
fn computer(view: &LlamaSetupView) -> Element<'_, Msg> {
    let mut rows: Vec<Element<'_, Msg>> = vec![
        row![
            style::mark(Health::Neutral),
            column![
                text("Memory and graphics card")
                    .font(style::semibold())
                    .size(style::BODY_SIZE),
                widgets::caption(view.computer.as_str()),
            ]
            .spacing(2)
            .width(Fill),
        ]
        .spacing(14)
        .align_y(Center)
        .padding(ROW_PADDING)
        .into(),
    ];
    let server: Element<'_, Msg> = if view.server_found {
        row![
            style::mark(Health::Ready),
            column![
                text("llama.cpp").font(style::semibold()).size(style::BODY_SIZE),
                widgets::caption(view.server_path.as_str()),
            ]
            .spacing(2)
            .width(Fill),
        ]
        .spacing(14)
        .align_y(Center)
        .padding(ROW_PADDING)
        .into()
    } else {
        row![
            style::mark(Health::Attention),
            column![
                text("llama.cpp").font(style::semibold()).size(style::BODY_SIZE),
                widgets::caption(format!("Not downloaded yet. This computer gets {}.", view.build_label)),
            ]
            .spacing(2)
            .width(Fill),
            button(text("Download llama.cpp").size(style::BODY_SIZE))
                .style(style::primary_button)
                .on_press_maybe((!view.busy).then_some(Msg::DownloadServer)),
        ]
        .spacing(14)
        .align_y(Center)
        .padding(ROW_PADDING)
        .into()
    };
    rows.push(server);
    // llama-server is here but can't use the graphics card: the right build replaces it.
    if let (true, Some(problem)) = (view.server_found, &view.card_problem) {
        rows.push(
            row![
                style::mark(Health::Attention),
                text(format!("{problem} Download it again to get {}.", view.build_label))
                    .size(style::CAPTION_SIZE)
                    .width(Fill),
                button(text("Download llama.cpp again").size(style::BODY_SIZE))
                    .style(style::standard_button)
                    .on_press_maybe((!view.busy).then_some(Msg::DownloadServer)),
            ]
            .spacing(14)
            .align_y(Center)
            .padding(ROW_PADDING)
            .into(),
        );
    }
    widgets::rows(rows.into_iter())
}

/// The running or last download: what, how far, and how it ended.
fn download_card(download: &llama_setup_service::DownloadView) -> Element<'_, Msg> {
    let mut body = column![
        text(download.what.as_str())
            .font(style::semibold())
            .size(style::BODY_SIZE)
    ]
    .spacing(8);
    if download.running {
        let percent = download.percent.unwrap_or(0) as f32;
        body = body
            .push(
                progress_bar(0.0..=PERCENT, percent)
                    .girth(BAR_HEIGHT)
                    .style(style::progress),
            )
            .push(widgets::caption(download.progress.as_str()));
    } else if let Some(why) = &download.why_failed {
        body = body.push(
            text(format!("It stopped: {why}"))
                .size(style::CAPTION_SIZE)
                .style(style::health_text(Health::Attention)),
        );
    } else if download.finished {
        body = body.push(
            text("Downloaded.")
                .size(style::CAPTION_SIZE)
                .style(style::health_text(Health::Ready)),
        );
    }
    container(body).padding(CARD_PADDING).width(Fill).into()
}

/// What a model row's button does.
#[derive(Clone, Copy, PartialEq)]
enum ModelAction {
    Use,
    Download,
}

/// A catalog model: its name, size and how it fits, and Use or Download.
fn model_row(model: &CatalogRow, action: ModelAction, busy: bool) -> Element<'_, Msg> {
    let mut name = row![text(model.label).font(style::semibold()).size(style::BODY_SIZE)].spacing(8);
    if model.recommended {
        name = name.push(
            container(text("Suggested").size(style::CAPTION_SIZE).color(style::ACCENT))
                .padding([1.0, 8.0])
                .style(style::badge),
        );
    }
    let (label, msg, enabled) = match action {
        ModelAction::Use => ("Use this model", Msg::UseModel(model.file), true),
        ModelAction::Download => ("Download", Msg::DownloadModel(model.file), !busy && !model.too_big),
    };
    let style = if model.recommended {
        style::primary_button
    } else {
        style::standard_button
    };
    row![
        column![name, widgets::caption(format!("{} · {}", model.size, model.fit))]
            .spacing(2)
            .width(Fill),
        button(text(label).size(style::BODY_SIZE))
            .style(style)
            .on_press_maybe(enabled.then_some(msg)),
    ]
    .spacing(14)
    .align_y(Center)
    .padding(ROW_PADDING)
    .into()
}
