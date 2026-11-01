//! The desktop window (iced): Match and Settings, with a status line for Laya and the app's own
//! model under them. Its screens print what the service layer gives the web pages (dashboard,
//! setup_steps), so the two can't disagree; what the window can't do yet opens the web page for
//! it in the browser, from the server that still runs beside it (for the Jobnet extension too).
//!
//! The window runs on the main thread, as the OS's windowing wants; the app's work runs on the
//! tokio runtime the server runs on (on_runtime), since the database and HTTP clients need it.

mod match_screen;
mod settings_screen;

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::futures::Stream;
use iced::widget::{button, column, container, row, rule, scrollable, text};
use iced::{Element, Fill, Font, Size, Task};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::context::Context;
use crate::dashboard::{self, Dashboard, Findings, Work};
use crate::job_scan::scan_service;
use crate::laya::laya_client::LayaState;

/// The window's title.
const TITLE: &str = "Recruiter";
/// The window's size when it opens, in logical pixels.
const WINDOW_SIZE: Size = Size::new(980.0, 720.0);
/// The smallest the window can be made.
const MIN_WINDOW_SIZE: Size = Size::new(560.0, 420.0);
/// How often the Match screen reads its status again, so a scan or a match saved in the browser
/// shows without a click.
const RELOAD_EVERY: Duration = Duration::from_secs(30);
/// Space between blocks on a screen.
const GAP: f32 = 12.0;
/// The window's padding.
const PADDING: f32 = 16.0;
/// The status line's text size.
const STATUS_SIZE: f32 = 13.0;

/// What the window needs from main: the app's work, the runtime it runs on, the server's address
/// (for the pages the window doesn't have yet), and what tells it to come to the front.
pub struct Launch {
    pub context: Arc<Context>,
    pub runtime: Handle,
    /// "http://127.0.0.1:8765/".
    pub address: String,
    /// A second start on this database (instance::front).
    pub front: UnboundedReceiver<()>,
}

/// The window's screens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Screen {
    Match,
    Settings,
}

/// What the Match screen shows, read together.
#[derive(Clone)]
pub struct Overview {
    pub dashboard: Dashboard,
    pub findings: Findings,
}

/// What a reload read.
#[derive(Clone)]
pub struct Read {
    overview: Result<Overview, String>,
    status: Vec<String>,
}

/// The window's state.
pub struct Shell {
    context: Arc<Context>,
    runtime: Handle,
    address: String,
    screen: Screen,
    /// None until first read; Err says why it couldn't be.
    overview: Option<Result<Overview, String>>,
    /// The status line, read with the overview (it looks in the llama folder, too slow for
    /// every redraw).
    status: Vec<String>,
    /// The last thing a click said (Scan now's answer, a page that couldn't open).
    said: Option<String>,
}

/// Clone, as iced's buttons need; a reload's answer is shared rather than copied.
#[derive(Clone)]
pub enum Message {
    Show(Screen),
    Reload,
    Loaded(Arc<Read>),
    /// Opens one of the web pages (its path under the server's address) in the browser.
    OpenPage(String),
    Do(Work),
    Said(String),
    /// A second start asked for the window.
    Front,
}

/// Opens the window and runs until it is closed.
pub fn run(launch: Launch) -> iced::Result {
    // boot may be called more than once by iced's types, but runs once; the channel's end is
    // taken by that run.
    let launch = Mutex::new(Some(launch));
    iced::application(move || boot(&launch), update, view)
        .title(TITLE)
        .window(window_settings())
        .default_font(default_font())
        .run()
}

/// A small window saying why the app couldn't start, for a start without a console to say it
/// in (the window is how a double-clicked Recruiter talks).
pub fn show_failure(why: String) -> iced::Result {
    iced::application(move || Failure(why.clone()), |_: &mut Failure, _: ()| {}, failure_view)
        .title(TITLE)
        .window_size(Size::new(560.0, 220.0))
        .default_font(default_font())
        .run()
}

/// Why the app couldn't start, as the failure window's state.
struct Failure(String);

fn failure_view(failure: &Failure) -> Element<'_, ()> {
    column![
        text("Recruiter couldn't start").size(20),
        text(failure.0.as_str()),
        text("This is in the log too.").size(STATUS_SIZE),
    ]
    .spacing(GAP)
    .padding(PADDING)
    .into()
}

fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: WINDOW_SIZE,
        min_size: Some(MIN_WINDOW_SIZE),
        ..iced::window::Settings::default()
    }
}

/// Windows 11's own font; elsewhere the system's sans-serif.
fn default_font() -> Font {
    if cfg!(windows) {
        Font::with_name("Segoe UI")
    } else {
        Font::DEFAULT
    }
}

fn boot(launch: &Mutex<Option<Launch>>) -> (Shell, Task<Message>) {
    let Launch {
        context,
        runtime,
        address,
        front,
    } = launch
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
        .expect("the window boots once");
    let ticks = ticks(&runtime);
    let shell = Shell {
        context,
        runtime,
        address,
        screen: Screen::Match,
        overview: None,
        status: Vec::new(),
        said: None,
    };
    let tasks = Task::batch([
        Task::done(Message::Reload),
        Task::run(received(front), |()| Message::Front),
        Task::run(received(ticks), |()| Message::Reload),
    ]);
    (shell, tasks)
}

/// A tick every RELOAD_EVERY, from a timer on the runtime (iced's own executor has no timers).
fn ticks(runtime: &Handle) -> UnboundedReceiver<()> {
    let (sender, receiver) = unbounded_channel();
    runtime.spawn(async move {
        loop {
            tokio::time::sleep(RELOAD_EVERY).await;
            if sender.send(()).is_err() {
                break;
            }
        }
    });
    receiver
}

/// What receiver gets, as a stream for a Task.
fn received<T: Send + 'static>(receiver: UnboundedReceiver<T>) -> impl Stream<Item = T> + Send + 'static {
    iced::futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    })
}

/// Runs work on the app's runtime and gives its answer to done; a panic in it is said.
fn on_runtime<T: Send + 'static>(
    runtime: &Handle,
    work: impl Future<Output = T> + Send + 'static,
    done: impl Fn(T) -> Message + Send + 'static,
) -> Task<Message> {
    let job = runtime.spawn(work);
    Task::perform(job, move |answer| match answer {
        Ok(answer) => done(answer),
        Err(e) => {
            crate::log_line!("The window's work stopped: {e}");
            Message::Said(format!("That stopped with an error: {e}"))
        }
    })
}

/// The Match screen's status and saved matches, and the status line.
async fn read(context: Arc<Context>) -> Arc<Read> {
    Arc::new(Read {
        overview: overview(&context).await,
        status: vec![laya_status(&context), local_model_status(&context)],
    })
}

async fn overview(context: &Context) -> Result<Overview, String> {
    let dashboard = dashboard::dashboard(context).await?;
    let findings = dashboard::findings(context).await?;
    Ok(Overview { dashboard, findings })
}

fn update(shell: &mut Shell, message: Message) -> Task<Message> {
    match message {
        Message::Show(screen) => shell.screen = screen,
        Message::Reload => {
            return on_runtime(&shell.runtime, read(Arc::clone(&shell.context)), Message::Loaded);
        }
        Message::Loaded(read) => {
            let read = Arc::unwrap_or_clone(read);
            if let Err(why) = &read.overview {
                crate::log_line!("The window's status couldn't be read: {why}");
            }
            shell.overview = Some(read.overview);
            shell.status = read.status;
        }
        Message::OpenPage(path) => {
            let url = format!("{}{path}", shell.address);
            if let Err(e) = webbrowser::open(&url) {
                crate::log_line!("Couldn't open {url} in the browser: {e}");
                shell.said = Some(format!("Couldn't open the browser ({e}). Open {url} in it."));
            }
        }
        Message::Do(Work::ScanNow) => {
            let scan = scan_service::scan_now(Arc::clone(&shell.context));
            return on_runtime(&shell.runtime, scan, |said| {
                Message::Said(said.map_or_else(|e| e.message, str::to_string))
            })
            .chain(Task::done(Message::Reload));
        }
        Message::Said(said) => shell.said = Some(said),
        Message::Front => {
            return iced::window::latest().and_then(|window| {
                // Out of the taskbar first, when it was minimised there.
                iced::window::minimize(window, false).chain(iced::window::gain_focus(window))
            });
        }
    }
    Task::none()
}

fn view(shell: &Shell) -> Element<'_, Message> {
    let tabs = row![
        tab("Match", Screen::Match, shell.screen),
        tab("Settings", Screen::Settings, shell.screen),
    ]
    .spacing(GAP);
    let screen = match shell.screen {
        Screen::Match => match_screen::view(shell.overview.as_ref()),
        Screen::Settings => settings_screen::view(&shell.context),
    };
    let mut page = column![tabs, rule::horizontal(1)].spacing(GAP);
    if let Some(said) = &shell.said {
        page = page.push(container(text(said.as_str())).padding(8).style(container::rounded_box));
    }
    page.push(scrollable(container(screen).width(Fill).padding([0.0, PADDING])).height(Fill))
        .push(rule::horizontal(1))
        .push(status_line(&shell.status))
        .padding(PADDING)
        .into()
}

fn tab(label: &str, screen: Screen, current: Screen) -> Element<'_, Message> {
    let style = if screen == current {
        button::primary
    } else {
        button::secondary
    };
    button(label).style(style).on_press(Message::Show(screen)).into()
}

/// Laya and the app's own model, as they stood at the last reload.
fn status_line(status: &[String]) -> Element<'_, Message> {
    iced::widget::Row::with_children(status.iter().map(|part| text(part.as_str()).size(STATUS_SIZE).into()))
        .spacing(GAP * 2.0)
        .into()
}

fn laya_status(context: &Context) -> String {
    match context.laya.as_ref().map(|laya| laya.state()) {
        None => "Laya: no model".to_string(),
        Some(LayaState::Loading) => "Laya: loading".to_string(),
        Some(LayaState::Loaded) => "Laya: ready".to_string(),
        Some(LayaState::Failed(_)) => "Laya: couldn't load (see the log)".to_string(),
    }
}

fn local_model_status(context: &Context) -> String {
    let Some(llama) = &context.llama else {
        return "Local model: none".to_string();
    };
    if let Some(model) = llama.running_model() {
        return format!("Local model: {model}, running");
    }
    if llama.server_path().is_none() {
        return "Local model: not set up".to_string();
    }
    match llama.overview().chosen {
        Ok(model) => format!("Local model: {model}, starts when used"),
        Err(_) => "Local model: no model fits".to_string(),
    }
}
