//! The desktop window (iced): Match, the settings screens (Language models, Local model, Job
//! searches) and Settings, with a status line for Laya and the app's own
//! model under them. Its screens print what the service layer gives the web pages (dashboard,
//! setup_steps), so the two can't disagree; what the window can't do yet opens the web page for
//! it in the browser, from the server that still runs beside it (for the Jobnet extension too).
//!
//! The window runs on the main thread, as the OS's windowing wants; the app's work runs on the
//! tokio runtime the server runs on (on_runtime), since the database and HTTP clients need it.

mod job_searches;
mod language_models;
mod local_model;
mod match_screen;
mod matching;
mod settings_screen;
mod style;
mod widgets;

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use iced::futures::Stream;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{Column, Row, button, column, container, row, scrollable, space, text};
use iced::{Center, Element, Fill, Size, Task};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use crate::context::Context;
use crate::dashboard::{self, Dashboard, Findings, Health, Work};
use crate::job_scan::scan_service;
use crate::laya::laya_client::LayaState;

/// The window's title.
const TITLE: &str = "JobFinder";
/// The window's size when it opens, in logical pixels.
const WINDOW_SIZE: Size = Size::new(980.0, 720.0);
/// The failure window's size.
const FAILURE_WINDOW_SIZE: Size = Size::new(560.0, 240.0);
/// The smallest the window can be made.
const MIN_WINDOW_SIZE: Size = Size::new(560.0, 420.0);
/// How often the Match screen reads its status again, so a scan or a match saved in the browser
/// shows without a click.
const RELOAD_EVERY: Duration = Duration::from_secs(30);
/// Space between blocks on a screen.
const GAP: f32 = 12.0;
/// The window's padding.
const PADDING: f32 = 16.0;

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
    LanguageModels,
    LocalModel,
    JobSearches,
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
    status: Vec<(Health, String)>,
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
    status: Vec<(Health, String)>,
    /// The last thing a click said (Scan now's answer, a page that couldn't open).
    said: Option<String>,
    language_models: language_models::State,
    local_model: local_model::State,
    job_searches: job_searches::State,
    matching: matching::State,
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
    LanguageModels(language_models::Msg),
    LocalModel(local_model::Msg),
    JobSearches(job_searches::Msg),
    Matching(matching::Msg),
}

/// Opens the window and runs until it is closed.
pub fn run(launch: Launch) -> iced::Result {
    // boot may be called more than once by iced's types, but runs once; the channel's end is
    // taken by that run.
    let launch = Mutex::new(Some(launch));
    iced::application(move || boot(&launch), update, view)
        .title(TITLE)
        .window(window_settings())
        .default_font(style::font())
        .theme(style::theme())
        .run()
}

/// A small window saying why the app couldn't start, for a start without a console to say it
/// in (the window is how a double-clicked JobFinder talks).
pub fn show_failure(why: String) -> iced::Result {
    iced::application(move || Failure(why.clone()), |_: &mut Failure, _: ()| {}, failure_view)
        .title(TITLE)
        .window_size(FAILURE_WINDOW_SIZE)
        .default_font(style::font())
        .theme(style::theme())
        .run()
}

/// Why the app couldn't start, as the failure window's state.
struct Failure(String);

fn failure_view(failure: &Failure) -> Element<'_, ()> {
    let message = column![
        row![
            style::mark(Health::Attention),
            text("JobFinder couldn't start")
                .font(style::semibold())
                .size(style::SECTION_SIZE),
        ]
        .spacing(GAP)
        .align_y(Center),
        text(failure.0.as_str()).size(style::BODY_SIZE),
        text("This is in the log too.")
            .size(style::CAPTION_SIZE)
            .style(style::secondary),
    ]
    .spacing(GAP);
    container(container(message).padding(PADDING * 1.5).width(Fill).style(style::card))
        .padding(PADDING)
        .width(Fill)
        .height(Fill)
        .style(style::page)
        .into()
}

fn window_settings() -> iced::window::Settings {
    iced::window::Settings {
        size: WINDOW_SIZE,
        min_size: Some(MIN_WINDOW_SIZE),
        ..iced::window::Settings::default()
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
        language_models: language_models::State::default(),
        local_model: local_model::State::default(),
        job_searches: job_searches::State::default(),
        matching: matching::State::default(),
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

impl Shell {
    fn runner(&self) -> Runner {
        Runner {
            context: Arc::clone(&self.context),
            runtime: self.runtime.clone(),
        }
    }
}

/// Runs a screen's work on the app's runtime, with the app's work (Context).
pub struct Runner {
    context: Arc<Context>,
    runtime: Handle,
}

impl Runner {
    /// Runs work(context) on the runtime and gives its answer to done: Err when the work
    /// panicked (logged). work is called on the runtime too, not on the window's thread, which
    /// has no runtime: a timer or a client made there would panic.
    pub fn run<T, F, M>(
        &self,
        work: impl FnOnce(Arc<Context>) -> F + Send + 'static,
        done: impl Fn(Result<T, String>) -> M + Send + 'static,
    ) -> Task<M>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        M: Send + 'static,
    {
        let job = spawn_work(&self.runtime, Arc::clone(&self.context), work);
        Task::perform(job, move |answer| {
            done(answer.map_err(|e| {
                crate::log_line!("The window's work stopped: {e}");
                format!("That stopped with an error: {e}")
            }))
        })
    }
}

/// A step of work run with Runner::run_saying: a line it says as it goes, or its answer.
pub enum Step<T> {
    Said(String),
    Done(Result<T, String>),
}

/// What work run with Runner::run_saying says its steps with.
pub type Sayer = Arc<dyn Fn(&str) + Send + Sync>;

impl Runner {
    /// As run, for work that says what it is doing as it goes: each line it says comes as
    /// Step::Said, then its answer as Step::Done.
    pub fn run_saying<T, F, M>(
        &self,
        work: impl FnOnce(Arc<Context>, Sayer) -> F + Send + 'static,
        step: impl Fn(Step<T>) -> M + Send + 'static,
    ) -> Task<M>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
        M: Send + 'static,
    {
        let (sender, receiver) = unbounded_channel();
        let says = sender.clone();
        let sayer: Sayer = Arc::new(move |line: &str| {
            // The window was closed: nobody to say it to.
            let _ = says.send(Step::Said(line.to_string()));
        });
        let job = spawn_work(&self.runtime, Arc::clone(&self.context), move |context| {
            work(context, sayer)
        });
        self.runtime.spawn(async move {
            let done = job.await.map_err(|e| {
                crate::log_line!("The window's work stopped: {e}");
                format!("That stopped with an error: {e}")
            });
            let _ = sender.send(Step::Done(done));
        });
        Task::run(received(receiver), step)
    }
}

/// Calls work(context) on the runtime and runs what it gives there.
fn spawn_work<T, F>(
    runtime: &Handle,
    context: Arc<Context>,
    work: impl FnOnce(Arc<Context>) -> F + Send + 'static,
) -> tokio::task::JoinHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    runtime.spawn(async move { work(context).await })
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
        Message::Show(screen) => {
            shell.screen = screen;
            shell.said = None;
            // Each visit reads the screen's data again, so it shows what the browser changed.
            match screen {
                Screen::LanguageModels => return Task::done(Message::LanguageModels(language_models::Msg::Load)),
                Screen::LocalModel => return Task::done(Message::LocalModel(local_model::Msg::Load)),
                Screen::JobSearches => {
                    shell.job_searches.show_list();
                    return Task::done(Message::JobSearches(job_searches::Msg::Load));
                }
                // The sidebar's Match is the overview, read again.
                Screen::Match => {
                    shell.matching.page = matching::Page::Overview;
                    return Task::done(Message::Reload);
                }
                Screen::Settings => {}
            }
        }
        Message::Matching(matching::Msg::OpenPage(path)) => return Task::done(Message::OpenPage(path)),
        Message::Matching(matching::Msg::Changed) => return Task::done(Message::Reload),
        Message::Matching(msg) => {
            let runner = shell.runner();
            return matching::update(&mut shell.matching, msg, &runner).map(Message::Matching);
        }
        Message::LanguageModels(language_models::Msg::ShowLocalModel) => {
            return Task::done(Message::Show(Screen::LocalModel));
        }
        Message::JobSearches(job_searches::Msg::OpenPage(path)) => {
            return Task::done(Message::OpenPage(path));
        }
        Message::JobSearches(msg) => {
            let runner = shell.runner();
            return job_searches::update(&mut shell.job_searches, msg, &runner).map(Message::JobSearches);
        }
        Message::LocalModel(local_model::Msg::ModelInUse {
            message,
            setup_unfinished,
        }) => {
            // As the web page goes on: to Setup while it is unfinished, else to where the
            // model's uses are picked, since the uses stay on the key they were on.
            if setup_unfinished {
                return Task::batch([
                    Task::done(Message::OpenPage("setup".to_string())),
                    Task::done(Message::Said(message)),
                ]);
            }
            return Task::done(Message::Show(Screen::LanguageModels))
                .chain(Task::done(Message::LanguageModels(language_models::Msg::Tell(message))));
        }
        Message::LocalModel(msg) => {
            let runner = shell.runner();
            return local_model::update(&mut shell.local_model, msg, &runner).map(Message::LocalModel);
        }
        Message::LanguageModels(msg) => {
            let runner = shell.runner();
            return language_models::update(&mut shell.language_models, msg, &runner).map(Message::LanguageModels);
        }
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
    let screen = match shell.screen {
        Screen::Match if shell.matching.page == matching::Page::Overview => match_screen::view(shell.overview.as_ref()),
        Screen::Match => matching::view(&shell.matching).map(Message::Matching),
        Screen::LanguageModels => language_models::view(&shell.language_models).map(Message::LanguageModels),
        Screen::LocalModel => local_model::view(&shell.local_model).map(Message::LocalModel),
        Screen::JobSearches => job_searches::view(&shell.job_searches).map(Message::JobSearches),
        Screen::Settings => settings_screen::view(&shell.context),
    };
    let mut page = Column::new().spacing(GAP).max_width(PAGE_MAX_WIDTH);
    if let Some(said) = &shell.said {
        page = page.push(
            container(text(said.as_str()).size(style::BODY_SIZE))
                .padding([10.0, 14.0])
                .width(Fill)
                .style(style::card),
        );
    }
    let page = page.push(screen);
    let main = column![
        scrollable(container(page).padding([PADDING, PADDING * 2.0]).center_x(Fill))
            .direction(Direction::Vertical(
                Scrollbar::new()
                    .width(SCROLLBAR_WIDTH)
                    .scroller_width(SCROLLBAR_WIDTH)
                    .margin(2),
            ))
            .height(Fill),
        status_line(&shell.status),
    ];
    container(row![sidebar(shell.screen), main])
        .width(Fill)
        .height(Fill)
        .style(style::page)
        .into()
}

/// The widest a page's cards get, so lines stay readable on a wide window.
const PAGE_MAX_WIDTH: f32 = 920.0;
/// The scrollbar's width: thin, as Windows 11's.
const SCROLLBAR_WIDTH: f32 = 6.0;
/// The sidebar's width.
const SIDEBAR_WIDTH: f32 = 200.0;
/// The accent strip's size beside the selected sidebar item.
const STRIP_SIZE: Size = Size::new(3.0, 16.0);

/// The app's name and its screens, as Windows 11's Settings has them.
fn sidebar(current: Screen) -> Element<'static, Message> {
    column![
        container(text(TITLE).font(style::semibold()).size(style::SECTION_SIZE)).padding([8.0, 12.0]),
        nav_item("Match", Screen::Match, current),
        nav_item("Language models", Screen::LanguageModels, current),
        nav_item("Local model", Screen::LocalModel, current),
        nav_item("Job searches", Screen::JobSearches, current),
        nav_item("Settings", Screen::Settings, current),
    ]
    .spacing(4)
    .padding(PADDING / 2.0)
    .width(SIDEBAR_WIDTH)
    .into()
}

fn nav_item(label: &'static str, screen: Screen, current: Screen) -> Element<'static, Message> {
    let selected = screen == current;
    let strip: Element<'static, Message> = if selected {
        container(space())
            .width(STRIP_SIZE.width)
            .height(STRIP_SIZE.height)
            .style(style::selected_strip)
            .into()
    } else {
        space().width(STRIP_SIZE.width).into()
    };
    let label = text(label)
        .size(style::BODY_SIZE)
        .font(if selected { style::semibold() } else { style::font() });
    button(row![strip, label].spacing(10).align_y(Center))
        .padding([8.0, 4.0])
        .width(Fill)
        .style(style::subtle_button(selected))
        .on_press(Message::Show(screen))
        .into()
}

/// Laya and the app's own model, as they stood at the last reload: a dot and a word each.
fn status_line(status: &[(Health, String)]) -> Element<'_, Message> {
    let parts = status.iter().map(|(health, part)| {
        row![
            style::dot(*health),
            text(part.as_str()).size(style::CAPTION_SIZE).style(style::secondary)
        ]
        .spacing(6)
        .align_y(Center)
        .into()
    });
    container(Row::with_children(parts).spacing(GAP * 2.0))
        .padding([8.0, PADDING * 2.0])
        .into()
}

fn laya_status(context: &Context) -> (Health, String) {
    match context.laya.as_ref().map(|laya| laya.state()) {
        None => (Health::Neutral, "Laya: no model".to_string()),
        Some(LayaState::Loading) => (Health::Neutral, "Laya: loading".to_string()),
        Some(LayaState::Loaded) => (Health::Ready, "Laya: ready".to_string()),
        Some(LayaState::Failed(_)) => (Health::Attention, "Laya: couldn't load (see the log)".to_string()),
    }
}

fn local_model_status(context: &Context) -> (Health, String) {
    let Some(llama) = &context.llama else {
        return (Health::Neutral, "Local model: none".to_string());
    };
    if let Some(model) = llama.running_model() {
        return (Health::Ready, format!("Local model: {model}, running"));
    }
    if llama.server_path().is_none() {
        return (Health::Neutral, "Local model: not set up".to_string());
    }
    match llama.overview().chosen {
        Ok(model) => (Health::Ready, format!("Local model: {model}, starts when used")),
        Err(_) => (Health::Attention, "Local model: no model fits".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::key_vault::KeyVault;

    #[test]
    fn a_screens_work_may_make_a_timer_from_a_thread_without_the_runtime() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let context = runtime.block_on(async {
            let db = crate::db::open_in_memory().await.unwrap();
            let user_id = crate::server::local_user(&db).await.unwrap();
            let taxonomy = Arc::new(crate::taxonomy::Taxonomy::load());
            Arc::new(Context::new(db, taxonomy, user_id, KeyVault::in_memory()))
        });
        let handle = runtime.handle().clone();
        // The window's thread: no runtime entered, as in run_window.
        let job =
            std::thread::spawn(move || spawn_work(&handle, context, |_| tokio::time::sleep(Duration::from_millis(1))))
                .join()
                .unwrap();
        runtime.block_on(job).unwrap();
    }
}
