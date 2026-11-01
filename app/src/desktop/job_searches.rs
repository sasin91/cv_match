//! The Job searches screen: the job scan's pages (job_scan) in the window. The list of searches,
//! a search with the posts it found, and the form that makes or edits one, all through
//! search_service and scan_service, the functions the web pages use.

use std::sync::Arc;
use std::time::Duration;

use iced::widget::{Column, button, checkbox, column, container, row, text, text_editor, text_input};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, CARD_PADDING, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::job_scan::found_post_card::FoundPostCard;
use crate::job_scan::job_scan_model::{FAILED, MATCHING, NEW};
use crate::job_scan::job_search_card::JobSearchCard;
use crate::job_scan::job_search_rules::{MAX_FIELD, MAX_KEYWORDS, MAX_NAME};
use crate::job_scan::job_search_summary::RunState;
use crate::job_scan::scan_service;
use crate::job_scan::search_service::{self, SaveError, SearchForm, SearchList, SearchPage};

/// How often a search's page looks again while its scan runs or its posts are matched, so they
/// show as found and matched.
const SCAN_POLL_EVERY: Duration = Duration::from_secs(5);
/// Looks in a row that changed nothing before the page stops looking (5 minutes): a post left
/// waiting (no profile, say) would otherwise keep it looking for as long as it is open.
const MAX_QUIET_POLLS: u32 = 60;
/// A form field's width.
const FIELD_WIDTH: f32 = 460.0;
/// The score field's width.
const SCORE_WIDTH: f32 = 100.0;
/// The sites box's height.
const SITES_HEIGHT: f32 = 76.0;
/// A form's text, its "1" for a ticked box.
const TICKED: &str = "1";

/// Which of the job scan's pages shows.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Page {
    #[default]
    List,
    /// A search and its posts.
    Search(i64),
    /// The form for a search, 0 for a new one.
    Form(i64),
}

/// The search form as typed.
#[derive(Default)]
struct FormState {
    name: String,
    keywords: String,
    location: String,
    remote_ok: bool,
    sites: text_editor::Content,
    tailor_from: String,
    active: bool,
    errors: Vec<String>,
    /// There is a profile to match with; without one the form says to read a CV first.
    has_profile: bool,
    /// Read, so the form shows.
    ready: bool,
}

impl FormState {
    fn of(form: &SearchForm, has_profile: bool) -> Self {
        FormState {
            name: form.name.clone(),
            keywords: form.keywords.clone(),
            location: form.location.clone(),
            remote_ok: form.remote_ok == TICKED,
            sites: text_editor::Content::with_text(&form.sites),
            tailor_from: form.tailor_from_score.clone(),
            active: form.active == TICKED,
            errors: Vec::new(),
            has_profile,
            ready: true,
        }
    }

    /// The form as search_service takes it.
    fn posted(&self) -> SearchForm {
        let ticked = |on: bool| if on { TICKED } else { "" }.to_string();
        SearchForm {
            name: self.name.clone(),
            keywords: self.keywords.clone(),
            location: self.location.clone(),
            remote_ok: ticked(self.remote_ok),
            sites: self.sites.text(),
            tailor_from_score: self.tailor_from.clone(),
            active: ticked(self.active),
        }
    }
}

#[derive(Default)]
pub struct State {
    page: Page,
    /// The list of searches; None until read, Err why it couldn't be.
    list: Option<Result<Arc<SearchList>, String>>,
    /// The search's page as last read (its posts are in posts).
    search: Option<Result<Arc<SearchPage>, String>>,
    /// The posts shown: the first batch, and the ones Show more added.
    posts: Vec<FoundPostCard>,
    /// Batches shown; a running scan's reload only reloads while there is one.
    batches: usize,
    /// Dismissed posts are listed too.
    dismissed: bool,
    form: FormState,
    /// The link typed in Add a job post.
    add_url: String,
    /// Delete was pressed once, waiting for the second press.
    confirm_delete: bool,
    /// A reload is on its way while a scan runs, so it isn't asked for twice.
    polling: bool,
    /// Reloads in a row that found the posts as they were.
    quiet_polls: u32,
    /// The last read asked for; an answer to an older one is dropped (is_stale).
    request: u64,
    /// Work is running, so the buttons wait.
    busy: bool,
    /// What the last click came to.
    said: Option<(Health, String)>,
}

impl State {
    /// Back to the list, as a visit from the sidebar starts.
    pub fn show_list(&mut self) {
        self.page = Page::List;
        self.said = None;
    }
}

/// What saving the form came to.
#[derive(Debug, Clone)]
pub enum Saved {
    Done(i64, &'static str),
    Invalid(Vec<String>),
    Failed(String),
}

#[derive(Clone)]
pub enum Msg {
    /// Reads the page showing again.
    Load,
    ListLoaded {
        request: u64,
        list: Result<Arc<SearchList>, String>,
    },
    /// A batch of a search's page: the first one, or one more when more.
    SearchLoaded {
        request: u64,
        page: Result<Arc<SearchPage>, String>,
        more: bool,
    },
    /// The form of search id (0 for a new one), with whether there is a profile.
    FormLoaded {
        request: u64,
        form: Result<(SearchForm, bool), String>,
    },
    Go(Page),
    ShowDismissed(bool),
    ShowMore,
    Name(String),
    Keywords(String),
    Location(String),
    RemoteOk(bool),
    Sites(text_editor::Action),
    TailorFrom(String),
    Active(bool),
    Save,
    Saved(Saved),
    AskDelete,
    CancelDelete,
    Delete(i64),
    ScanNow,
    AddUrl(String),
    Add(i64),
    Retry(i64),
    Dismiss {
        post: i64,
        undo: bool,
    },
    /// A job post's own page, in the browser.
    OpenPost(String),
    /// One of the web pages (a match, setup); the shell opens it.
    OpenPage(String),
    /// What a click came to; then the page goes to (and reads) then.
    Said {
        said: Result<String, String>,
        then: Page,
    },
    /// The next look at a running scan.
    Poll,
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    if is_stale(state.request, &msg) {
        return Task::none();
    }
    // A visit or a click can start new work (a scan, a match), so the page may look again for
    // it however long the last wait was.
    if restarts_looking(&msg) {
        state.quiet_polls = 0;
    }
    match msg {
        Msg::Load => return load(state, runner),
        Msg::Go(page) => {
            state.page = page;
            state.confirm_delete = false;
            state.said = None;
            if let Page::Search(_) = page {
                state.dismissed = false;
            }
            return load(state, runner);
        }
        Msg::ListLoaded { list, .. } => {
            state.list = Some(list);
            state.busy = false;
        }
        Msg::SearchLoaded { page, more, .. } => {
            state.busy = false;
            if let Ok(page) = &page {
                if more {
                    state.posts.extend(page.posts.iter().cloned());
                    state.batches += 1;
                } else {
                    state.quiet_polls = if state.posts == page.posts {
                        state.quiet_polls + 1
                    } else {
                        0
                    };
                    state.posts = page.posts.clone();
                    state.batches = 1;
                }
            }
            let working = page.as_ref().is_ok_and(|page| still_working(page));
            state.search = Some(page);
            let keep_looking = working && state.quiet_polls < MAX_QUIET_POLLS;
            if keep_looking && !state.polling && state.batches == 1 {
                state.polling = true;
                return poll_later(runner);
            }
        }
        Msg::Poll => {
            state.polling = false;
            if !matches!(state.page, Page::Search(_)) || state.batches != 1 {
                return Task::none();
            }
            // A read the user asked for (Show more, a dismiss) is on its way: a reload now
            // would make it stale and drop it, so the look waits a turn.
            if state.busy {
                state.polling = true;
                return poll_later(runner);
            }
            return load(state, runner);
        }
        Msg::FormLoaded {
            form: Ok((form, has_profile)),
            ..
        } => {
            state.form = FormState::of(&form, has_profile);
            state.busy = false;
        }
        Msg::FormLoaded { form: Err(why), .. } => {
            state.said = Some((Health::Attention, why));
            state.busy = false;
        }
        Msg::ShowDismissed(dismissed) => {
            state.dismissed = dismissed;
            return load(state, runner);
        }
        Msg::ShowMore => {
            let (Page::Search(id), Some(Ok(page))) = (state.page, &state.search) else {
                return Task::none();
            };
            let Some(after) = page.next_after.clone() else {
                return Task::none();
            };
            state.busy = true;
            state.request += 1;
            return search_batch(runner, state.request, id, state.dismissed, after);
        }
        Msg::Name(name) => state.form.name = name,
        Msg::Keywords(keywords) => state.form.keywords = keywords,
        Msg::Location(location) => state.form.location = location,
        Msg::RemoteOk(on) => state.form.remote_ok = on,
        Msg::Sites(action) => state.form.sites.perform(action),
        Msg::TailorFrom(score) => state.form.tailor_from = score,
        Msg::Active(on) => state.form.active = on,
        Msg::Save => {
            let Page::Form(id) = state.page else {
                return Task::none();
            };
            state.busy = true;
            let form = state.form.posted();
            return runner.run(
                move |context| async move {
                    match search_service::save(&context, id, &form).await {
                        Ok((id, said)) => Saved::Done(id, said),
                        Err(SaveError::Invalid(errors)) => Saved::Invalid(errors),
                        Err(SaveError::Failed(e)) => Saved::Failed(e.message),
                    }
                },
                |answer| Msg::Saved(answer.unwrap_or_else(Saved::Failed)),
            );
        }
        Msg::Saved(Saved::Done(id, said)) => {
            state.form.errors.clear();
            return Task::done(Msg::Said {
                said: Ok(said.to_string()),
                then: Page::Search(id),
            });
        }
        Msg::Saved(Saved::Invalid(errors)) => {
            state.form.errors = errors;
            state.busy = false;
        }
        Msg::Saved(Saved::Failed(why)) => {
            state.said = Some((Health::Attention, why));
            state.busy = false;
        }
        Msg::AskDelete => state.confirm_delete = true,
        Msg::CancelDelete => state.confirm_delete = false,
        Msg::Delete(id) => {
            state.confirm_delete = false;
            state.busy = true;
            return runner.run(
                move |context| async move { search_service::delete(&context, id).await.map(str::to_string) },
                said(Page::List),
            );
        }
        Msg::ScanNow => {
            state.busy = true;
            let page = state.page;
            return runner.run(
                |context| async move { scan_service::scan_now(context).await.map(str::to_string) },
                said(page),
            );
        }
        Msg::AddUrl(url) => state.add_url = url,
        Msg::Add(id) => {
            state.busy = true;
            let url = state.add_url.clone();
            return runner.run(
                move |context| async move { scan_service::add_post(context, id, &url).await.map(str::to_string) },
                said(state.page),
            );
        }
        Msg::Retry(post) => {
            state.busy = true;
            return runner.run(
                move |context| async move {
                    scan_service::retry(context, post)
                        .await
                        .map(|(_, said)| said.to_string())
                },
                said(state.page),
            );
        }
        Msg::Dismiss { post, undo } => {
            state.busy = true;
            let page = state.page;
            return runner.run(
                move |context| async move { search_service::dismiss(&context, post, undo).await },
                move |answer| match answer.and_then(|dismissed| dismissed.map_err(|e| e.message)) {
                    // The post changes in place; nothing to say.
                    Ok(_) => Msg::Load,
                    Err(why) => Msg::Said {
                        said: Err(why),
                        then: page,
                    },
                },
            );
        }
        Msg::OpenPost(url) => {
            if let Err(e) = webbrowser::open(&url) {
                crate::log_line!("Couldn't open {url} in the browser: {e}");
                state.said = Some((
                    Health::Attention,
                    format!("Couldn't open the browser ({e}). Open {url} in it."),
                ));
            }
        }
        // The shell opens it.
        Msg::OpenPage(_) => {}
        Msg::Said { said, then } => {
            if said.is_ok() {
                state.add_url.clear();
            }
            state.said = Some(match said {
                Ok(said) => (Health::Ready, said),
                Err(why) => (Health::Attention, why),
            });
            state.page = then;
            return load(state, runner);
        }
    }
    Task::none()
}

/// An answer to a read older than the last one asked for (another page, another search, the
/// other dismissed filter): showing it would put one search's form or posts under another, or
/// posts under the wrong filter.
fn is_stale(request: u64, msg: &Msg) -> bool {
    match msg {
        Msg::ListLoaded { request: asked, .. }
        | Msg::SearchLoaded { request: asked, .. }
        | Msg::FormLoaded { request: asked, .. } => *asked != request,
        _ => false,
    }
}

/// A service's answer as Msg::Said, going on to then.
fn said(
    then: Page,
) -> impl Fn(Result<Result<String, crate::service_error::ServiceError>, String>) -> Msg + Send + 'static {
    move |answer| Msg::Said {
        said: answer.and_then(|said| said.map_err(|e| e.message)),
        then,
    }
}

/// Reads the page showing.
fn load(state: &mut State, runner: &Runner) -> Task<Msg> {
    state.busy = true;
    state.request += 1;
    let request = state.request;
    match state.page {
        Page::List => runner.run(
            |context| async move { search_service::searches(&context).await.map(Arc::new) },
            move |read| Msg::ListLoaded {
                request,
                list: read.and_then(|list| list.map_err(|e| e.message)),
            },
        ),
        Page::Search(id) => search_batch(runner, request, id, state.dismissed, String::new()),
        Page::Form(id) => {
            state.form = FormState::default();
            runner.run(
                move |context| async move {
                    let form = if id == 0 {
                        SearchForm::new_search()
                    } else {
                        SearchForm::of(&search_service::search(&context, id).await?)
                    };
                    Ok((form, search_service::has_profile(&context).await?))
                },
                move |read: Result<Result<(SearchForm, bool), crate::service_error::ServiceError>, String>| {
                    Msg::FormLoaded {
                        request,
                        form: read.and_then(|form| form.map_err(|e| e.message)),
                    }
                },
            )
        }
    }
}

/// A batch of search id's posts, after the cursor after ("" for the first).
fn search_batch(runner: &Runner, request: u64, id: i64, dismissed: bool, after: String) -> Task<Msg> {
    let more = !after.is_empty();
    runner.run(
        move |context| async move {
            search_service::search_page(&context, id, dismissed, &after)
                .await
                .map(Arc::new)
        },
        move |read| Msg::SearchLoaded {
            request,
            page: read.and_then(|page| page.map_err(|e| e.message)),
            more,
        },
    )
}

/// A scan is running (this search's, or one that will reach it), or a post on the page is
/// still to be matched (a scan, Add or Try again matches in the background).
fn still_working(page: &SearchPage) -> bool {
    let scanning = page.scan_running
        || page
            .summary
            .last_scan
            .as_ref()
            .is_some_and(|scan| scan.state == RunState::Running);
    scanning
        || page
            .posts
            .iter()
            .any(|post| [NEW, MATCHING].contains(&post.status.as_str()))
}

/// A visit or a click whose answer reloads the page: Go, the dismissed filter, Dismiss, and
/// Said (Scan now, Add, Try again, a save).
fn restarts_looking(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::Go(_) | Msg::ShowDismissed(_) | Msg::Dismiss { .. } | Msg::Said { .. }
    )
}

/// A Poll after SCAN_POLL_EVERY.
fn poll_later(runner: &Runner) -> Task<Msg> {
    runner.run(|_| tokio::time::sleep(SCAN_POLL_EVERY), |_| Msg::Poll)
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let mut screen = Column::new().spacing(GAP * 1.5);
    if state.page != Page::List {
        screen = screen.push(back_button(state));
    }
    let body = match state.page {
        Page::List => list_view(state),
        Page::Search(_) => search_view(state),
        Page::Form(id) => form_view(state, id),
    };
    screen.push(body).into()
}

/// "‹ Job searches", or back to the search from its form.
fn back_button(state: &State) -> Element<'_, Msg> {
    let (label, to) = match state.page {
        Page::Form(id) if id != 0 => ("‹ Back to the search", Page::Search(id)),
        _ => ("‹ Job searches", Page::List),
    };
    button(text(label).size(style::BODY_SIZE))
        .padding([4.0, 8.0])
        .style(style::subtle_button(false))
        .on_press(Msg::Go(to))
        .into()
}

/// What the last click came to, under the title.
fn said_bar(state: &State) -> Option<Element<'_, Msg>> {
    state
        .said
        .as_ref()
        .map(|(health, said)| widgets::message_bar(*health, said.as_str()))
}

fn list_view(state: &State) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Job searches",
        widgets::note(
            "Save what you're looking for, and the job scan looks for new job posts for you. Every post it finds is matched with your profile and saved with your matches, and a good match gets a tailored résumé and application right away. It reads Jobnet and Jobindex itself and matches on the model you picked for evaluating job posts, which can be the one on this computer; other sites are searched with OpenAI's web search when your OpenAI key is picked.",
        ),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    screen = screen.extend(said_bar(state));
    let list = match &state.list {
        None => return screen.push(widgets::note("Reading your searches…")).into(),
        Some(Err(why)) => {
            return screen
                .push(widgets::message_bar(Health::Attention, why.as_str()))
                .into();
        }
        Some(Ok(list)) => list,
    };
    if let Some(problem) = &list.problem {
        screen = screen.push(widgets::message_bar(Health::Attention, problem.as_str()));
    }
    let mut actions = row![
        button(text("New job search").size(style::BODY_SIZE))
            .padding([8.0, 16.0])
            .style(style::primary_button)
            .on_press(Msg::Go(Page::Form(0)))
    ]
    .spacing(8);
    if !list.searches.is_empty() {
        actions = actions.push(
            button(text("Scan now").size(style::BODY_SIZE))
                .padding([8.0, 16.0])
                .style(style::standard_button)
                .on_press_maybe((!state.busy).then_some(Msg::ScanNow)),
        );
    }
    screen = screen.push(actions);
    let searches: Element<'_, Msg> = if list.searches.is_empty() {
        container(widgets::note("You have no job searches yet."))
            .padding(CARD_PADDING)
            .into()
    } else {
        widgets::rows(list.searches.iter().map(search_row))
    };
    screen.push(widgets::section("Your searches", searches)).into()
}

/// A saved search: its name and chip, what it looks for and found, opening its page.
fn search_row(search: &JobSearchCard) -> Element<'_, Msg> {
    let mut name = row![
        text(search.name.as_str())
            .font(style::semibold())
            .size(style::BODY_SIZE)
    ]
    .spacing(8)
    .align_y(Center);
    if let Some(chip) = &search.chip {
        let health = if chip.label == "Paused" {
            Health::Attention
        } else {
            Health::Neutral
        };
        name = name.push(chip_label(chip.label, health));
    }
    button(
        row![
            column![name, widgets::caption(search.meta.as_str())]
                .spacing(2)
                .width(Fill),
            text("›").size(style::SECTION_SIZE).style(style::secondary),
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(ROW_PADDING)
    .width(Fill)
    .style(style::row_button)
    .on_press(Msg::Go(Page::Search(search.id)))
    .into()
}

/// A small pill: a chip, a status or a score.
fn chip_label(label: &str, health: Health) -> Element<'_, Msg> {
    container(text(label).size(style::CAPTION_SIZE))
        .padding([1.0, 8.0])
        .style(style::chip(health))
        .into()
}

fn search_view(state: &State) -> Element<'_, Msg> {
    let page = match &state.search {
        None => return widgets::note("Reading the search…"),
        Some(Err(why)) => return widgets::message_bar(Health::Attention, why.as_str()),
        Some(Ok(page)) => page,
    };
    let search = &page.search;
    let mut screen = Column::new().spacing(GAP * 1.5).push(widgets::page_title(
        search.name.as_str(),
        widgets::note(page.summary.line.as_str()),
    ));
    screen = screen.extend(said_bar(state));
    if !search.active {
        screen = screen.push(widgets::message_bar(
            Health::Neutral,
            "This search is off, so it isn't scanned. Turn it on under Edit.",
        ));
    } else if !search.paused_reason.is_empty() {
        screen = screen.push(widgets::message_bar(Health::Attention, search.paused_reason.as_str()));
    } else if let Some(problem) = &page.problem {
        screen = screen.push(widgets::message_bar(Health::Attention, problem.as_str()));
    }
    if let Some(last) = &page.summary.last_scan {
        let mut line = row![widgets::caption(format!("Last scan {}:", last.when))].spacing(4);
        line = line.push(if last.is_error() {
            text(last.text.as_str())
                .size(style::CAPTION_SIZE)
                .style(style::health_text(Health::Attention))
                .into()
        } else {
            widgets::caption(last.text.as_str())
        });
        screen = screen.push(line.push(widgets::caption(last.usage.as_str())).wrap());
    }
    let waiting = |msg: Msg| (!state.busy).then_some(msg);
    let mut actions = row![].spacing(8).align_y(Center);
    if search.active {
        actions = actions.push(
            button(text("Scan now").size(style::BODY_SIZE))
                .padding([8.0, 16.0])
                .style(style::primary_button)
                .on_press_maybe(waiting(Msg::ScanNow)),
        );
    }
    actions = actions.push(
        button(text("Edit").size(style::BODY_SIZE))
            .padding([8.0, 16.0])
            .style(style::standard_button)
            .on_press(Msg::Go(Page::Form(search.id))),
    );
    actions = actions.push(
        checkbox(state.dismissed)
            .label("Show dismissed posts")
            .text_size(style::BODY_SIZE)
            .style(style::check_box)
            .on_toggle(Msg::ShowDismissed),
    );
    screen = screen.push(actions);
    screen = screen.push(widgets::section("Add a job post", add_post_card(state, search.id)));

    let posts: Element<'_, Msg> = if state.posts.is_empty() {
        container(widgets::note("No job posts found yet."))
            .padding(CARD_PADDING)
            .into()
    } else {
        widgets::rows(state.posts.iter().map(|post| post_row(post, state.busy)))
    };
    screen = screen.push(widgets::section("Posts found", posts));
    if page.next_after.is_some() {
        screen = screen.push(
            button(text("Show more").size(style::BODY_SIZE))
                .padding([8.0, 16.0])
                .style(style::standard_button)
                .on_press_maybe(waiting(Msg::ShowMore)),
        );
    }
    screen.into()
}

/// Add a job post by its link, and match it.
fn add_post_card(state: &State, id: i64) -> Element<'_, Msg> {
    let add = (!state.busy && !state.add_url.trim().is_empty()).then_some(Msg::Add(id));
    container(
        column![
            row![
                text_input("https://…", &state.add_url)
                    .on_input(Msg::AddUrl)
                    .on_submit_maybe(add.clone())
                    .size(style::BODY_SIZE)
                    .padding(8)
                    .style(style::text_field)
                    .width(Fill),
                button(text("Add and match").size(style::BODY_SIZE))
                    .padding([8.0, 16.0])
                    .style(style::primary_button)
                    .on_press_maybe(add),
            ]
            .spacing(8)
            .align_y(Center),
            widgets::caption("A post the scan missed: paste its link, and it's matched like one it found."),
        ]
        .spacing(8),
    )
    .padding(CARD_PADDING)
    .into()
}

/// A post the search found: what it is and how it matched, and its buttons.
fn post_row(post: &FoundPostCard, busy: bool) -> Element<'_, Msg> {
    let chip = match &post.score {
        Some(score) => chip_label_owned(
            format!("{} · {}", score.percent, score.tag_name),
            score_health(&score.tag),
        ),
        None => chip_label_owned(
            post.status_name.clone(),
            if post.status == FAILED {
                Health::Attention
            } else {
                Health::Neutral
            },
        ),
    };
    let mut about = column![
        text(post.heading.as_str())
            .font(style::semibold())
            .size(style::BODY_SIZE),
        widgets::caption(post.meta.as_str()),
    ]
    .spacing(2)
    .width(Fill);
    for warning in [&post.disqualifier, &post.error] {
        if !warning.is_empty() {
            about = about.push(
                text(warning.as_str())
                    .size(style::CAPTION_SIZE)
                    .style(style::health_text(Health::Attention)),
            );
        }
    }
    let waiting = |msg: Msg| (!busy).then_some(msg);
    let mut buttons = row![].spacing(8);
    if let Some(link) = &post.match_link {
        buttons = buttons.push(
            button(text(link.label).size(style::BODY_SIZE))
                .style(style::primary_button)
                .on_press(Msg::OpenPage(link.url.clone())),
        );
    }
    if post.can_retry {
        buttons = buttons.push(
            button(text("Try again").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press_maybe(waiting(Msg::Retry(post.id))),
        );
    }
    buttons = buttons
        .push(
            button(text(post.dismiss_label).size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press_maybe(waiting(Msg::Dismiss {
                    post: post.id,
                    undo: !post.dismiss_undo.is_empty(),
                })),
        )
        .push(
            button(text("Open the post ↗").size(style::BODY_SIZE))
                .style(style::subtle_button(false))
                .on_press(Msg::OpenPost(post.url.clone())),
        );
    column![row![about, chip].spacing(14), buttons.wrap()]
        .spacing(10)
        .padding(ROW_PADDING)
        .into()
}

/// chip_label for a label made here.
fn chip_label_owned(label: String, health: Health) -> Element<'static, Msg> {
    container(text(label).size(style::CAPTION_SIZE))
        .padding([1.0, 8.0])
        .style(style::chip(health))
        .into()
}

/// A score's tag as a health: a top or good match is ready, a weak one needs a look.
fn score_health(tag: &str) -> Health {
    match tag {
        "top" | "good" => Health::Ready,
        "poor" => Health::Attention,
        _ => Health::Neutral,
    }
}

fn form_view(state: &State, id: i64) -> Element<'_, Msg> {
    let title = if id == 0 { "New job search" } else { "Edit job search" };
    let mut screen = Column::new().spacing(GAP * 1.5).push(widgets::page_title(
        title,
        widgets::note("What the job scan looks for, where, and when a match is tailored."),
    ));
    screen = screen.extend(said_bar(state));
    let form = &state.form;
    if !form.ready {
        return screen.push(widgets::note("Reading the search…")).into();
    }
    let mut fields = Column::new()
        .spacing(14)
        .padding(CARD_PADDING)
        .push(widgets::field(
            "Name",
            input("Backend jobs in Odense", &form.name, MAX_NAME, Msg::Name),
            None,
        ))
        .push(widgets::field(
            "What to look for",
            input(
                "PHP udvikler, backend developer",
                &form.keywords,
                MAX_KEYWORDS,
                Msg::Keywords,
            ),
            Some("Job titles or keywords, separated by commas."),
        ))
        .push(widgets::field(
            "Where",
            input("Odense, or a postcode", &form.location, MAX_FIELD, Msg::Location),
            None,
        ))
        .push(
            checkbox(form.remote_ok)
                .label("Remote jobs too")
                .text_size(style::BODY_SIZE)
                .style(style::check_box)
                .on_toggle(Msg::RemoteOk),
        )
        .push(widgets::field(
            "Only these sites (optional)",
            text_editor(&form.sites)
                .placeholder("jobnet.dk\njobindex.dk")
                .on_action(Msg::Sites)
                .size(style::BODY_SIZE)
                .padding(8)
                .height(SITES_HEIGHT)
                .style(style::text_area)
                .width(FIELD_WIDTH)
                .into(),
            Some("One site per line. Leave it empty to look on the usual Danish job sites and company career pages."),
        ));
    fields = fields.push(profile_note(form.has_profile));
    fields = fields
        .push(widgets::field(
            "Tailor a résumé and application from this score",
            text_input("60", &form.tailor_from)
                .on_input(Msg::TailorFrom)
                .size(style::BODY_SIZE)
                .padding(8)
                .style(style::text_field)
                .width(SCORE_WIDTH)
                .into(),
            Some("0 to 100. Matches scoring at least this get a tailored résumé and application right away; 60 is a medium match, 80 a good one, 0 tailors every match."),
        ))
        .push(
            column![
                checkbox(form.active)
                    .label("Scan this search")
                    .text_size(style::BODY_SIZE)
                    .style(style::check_box)
                    .on_toggle(Msg::Active),
                widgets::caption("Searches are scanned on the job scan's schedule, set in Setup, or now with Scan now."),
            ]
            .spacing(6),
        )
        .extend((!form.errors.is_empty()).then(|| widgets::errors(&form.errors)))
        .push(
            button(text("Save job search").size(style::BODY_SIZE))
                .padding([8.0, 16.0])
                .style(style::primary_button)
                .on_press_maybe((!state.busy).then_some(Msg::Save)),
        );
    screen = screen.push(widgets::card(fields.into()));
    if id != 0 {
        screen = screen.push(widgets::section("Delete it", delete_card(state, id)));
    }
    screen.into()
}

/// A one-line text box of at most max characters.
fn input<'a>(placeholder: &'a str, value: &'a str, max: usize, on_input: fn(String) -> Msg) -> Element<'a, Msg> {
    text_input(placeholder, value)
        .on_input(move |typed| on_input(typed.chars().take(max).collect()))
        .size(style::BODY_SIZE)
        .padding(8)
        .style(style::text_field)
        .width(FIELD_WIDTH)
        .into()
}

/// Posts are matched with the profile; without one, read a CV first.
fn profile_note(has_profile: bool) -> Element<'static, Msg> {
    let said = if has_profile {
        "Every post is matched with your profile as it is when the post is found."
    } else {
        "Every post is matched with your profile as it is when the post is found. You have none yet: read your CV first."
    };
    let mut note = row![text(said).size(style::CAPTION_SIZE).width(Fill)]
        .spacing(12)
        .align_y(Center);
    let health = if has_profile {
        Health::Neutral
    } else {
        note = note.push(
            button(text("Read your CV in Setup").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(Msg::OpenPage("setup#cv".to_string())),
        );
        Health::Attention
    };
    container(row![style::mark(health), note].spacing(12).align_y(Center))
        .padding([10.0, 12.0])
        .width(Fill)
        .style(if has_profile { style::card } else { style::warning_bar })
        .into()
}

/// Delete, behind a second press.
fn delete_card(state: &State, id: i64) -> Element<'_, Msg> {
    let buttons = if state.confirm_delete {
        row![
            button(text("Cancel").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(Msg::CancelDelete),
            button(text("Delete this job search").size(style::BODY_SIZE))
                .style(style::danger_button)
                .on_press_maybe((!state.busy).then_some(Msg::Delete(id))),
        ]
    } else {
        row![
            button(text("Delete job search").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(Msg::AskDelete)
        ]
    };
    row![
        text("The posts it found go too. Their saved matches stay in your match lists.")
            .size(style::CAPTION_SIZE)
            .style(style::secondary)
            .width(Fill),
        buttons.spacing(8),
    ]
    .spacing(14)
    .align_y(Center)
    .padding(ROW_PADDING)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_to_an_older_read_is_dropped() {
        let form = |request| Msg::FormLoaded {
            request,
            form: Err(String::new()),
        };
        let posts = |request| Msg::SearchLoaded {
            request,
            page: Err(String::new()),
            more: false,
        };
        let list = |request| Msg::ListLoaded {
            request,
            list: Err(String::new()),
        };
        assert!(!is_stale(2, &form(2)));
        assert!(is_stale(3, &form(2)));
        assert!(!is_stale(2, &posts(2)));
        assert!(is_stale(3, &posts(2)));
        assert!(is_stale(3, &list(2)));
        assert!(!is_stale(3, &Msg::Load));
    }

    #[test]
    fn a_visit_or_a_click_looks_again_after_a_long_wait() {
        assert!(restarts_looking(&Msg::Go(Page::Search(1))));
        assert!(restarts_looking(&Msg::Said {
            said: Ok(String::new()),
            then: Page::Search(1),
        }));
        assert!(!restarts_looking(&Msg::Poll));
        assert!(!restarts_looking(&Msg::Load));
    }
}
