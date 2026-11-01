//! Matching in the window: a job post (a link, its text or a file) matched with the CV (the
//! profile from setup, or another one pasted or picked), its result with the verdicts, and the
//! saved matches by list, each opening its result, with Not interested and Bring back. All of
//! it through match_service, the work the match page does, so the two can't disagree.
//!
//! What the window can't do yet (writing the application and résumé, the Apply dialog, the
//! Jobnet joblog) opens the saved match's page in the browser.

use std::sync::Arc;

use iced::widget::{Column, Row, button, checkbox, column, container, row, text, text_editor, text_input};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, CARD_PADDING, ROW_PADDING};
use super::{GAP, Runner, Step};
use crate::context::Context;
use crate::dashboard::Health;
use crate::matches::match_model::LISTS;
use crate::matches::match_result::{MatchResult, Section};
use crate::matches::match_service::{self, MatchAnswer, MatchAsk, ReadCv, RejectOutcome, SavedMatchList};
use crate::matches::page_reader::Page as Fetched;
use crate::matches::reject_button::{self, RejectButton};
use crate::matches::reject_reason::{self, Rejection};
use crate::matches::saved_match_row::SavedMatchRow;
use crate::matches::verdict_row::VerdictRow;
use crate::profile;
use crate::service_error::{ErrorKind, ServiceError};

/// The CV box's height.
const CV_HEIGHT: f32 = 180.0;
/// The job box's height.
const JOB_HEIGHT: f32 = 220.0;
/// The score's size beside the job's name.
const SCORE_SIZE: f32 = 40.0;
/// The filter that shows every requirement (match_result::filters).
const ALL: &str = "all";
/// The files the Choose file dialog offers.
const FILE_KINDS: [&str; 4] = ["pdf", "txt", "md", "text"];
/// How a PDF starts, for one picked with another name.
const PDF_START: &[u8] = b"%PDF";

/// The Match screen's pages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Page {
    /// The status and the glance at the saved matches (match_screen).
    Overview,
    /// Match a job.
    New,
    /// A list of saved matches (match_model::LISTS).
    List(&'static str),
    /// A saved match's result.
    Saved(i64),
}

/// Which box a picked file goes in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Cv,
    Job,
}

/// A result on screen, with its filter and its Not interested form.
struct Shown {
    result: MatchResult,
    /// The filter key shown: ALL, or a verdict.
    filter: &'static str,
    /// The Not interested form is open.
    rejecting: bool,
    /// The reasons ticked, in reject_reason::REASONS's order.
    ticked: Vec<&'static str>,
    /// Each reason's note.
    notes: Vec<(&'static str, String)>,
    /// What the form says is missing.
    problem: Option<String>,
}

impl Shown {
    fn new(result: MatchResult) -> Self {
        Shown {
            result,
            filter: ALL,
            rejecting: false,
            ticked: Vec::new(),
            notes: Vec::new(),
            problem: None,
        }
    }

    fn note(&self, reason: &str) -> &str {
        self.notes
            .iter()
            .find(|(of, _)| *of == reason)
            .map_or("", |(_, note)| note.as_str())
    }
}

/// The CV a match is made with.
#[derive(Default)]
struct Cv {
    /// The profile from setup as text: None until read, Some(None) when there is none.
    profile: Option<Option<String>>,
    /// Another CV is pasted or picked rather than the profile.
    own: bool,
    text: text_editor::Content,
    /// The file it was picked from.
    file_name: Option<String>,
    /// The CV as read the last time, used again while its text is the same.
    read: Option<ReadCv>,
}

impl Cv {
    /// The text matched: the profile, or the CV in the box.
    fn text(&self) -> String {
        match (&self.profile, self.own) {
            (Some(Some(profile)), false) => profile.clone(),
            _ => self.text.text(),
        }
    }

    fn uses_profile(&self) -> bool {
        !self.own && matches!(self.profile, Some(Some(_)))
    }
}

#[derive(Default)]
struct List {
    /// None until read; Err says why it couldn't be.
    read: Option<Result<(), String>>,
    rows: Vec<SavedMatchRow>,
    next_after: Option<String>,
    empty: &'static str,
}

pub struct State {
    pub page: Page,
    cv: Cv,
    job_url: String,
    job: text_editor::Content,
    job_file: Option<String>,
    /// Matching, fetching or reading a file: the form's buttons wait.
    busy: bool,
    /// The step running, or what the last one came to.
    said: Option<(Health, String)>,
    /// The result of the last match on the Match a job page.
    fresh: Option<Shown>,
    list: List,
    /// The saved match open on its page.
    saved: Option<Result<Shown, String>>,
    /// Where a saved match's page goes back to.
    back: Page,
    /// The number of the newest read, so an answer to an older one is dropped.
    request: u64,
}

impl Default for State {
    fn default() -> Self {
        State {
            page: Page::Overview,
            cv: Cv::default(),
            job_url: String::new(),
            job: text_editor::Content::new(),
            job_file: None,
            busy: false,
            said: None,
            fresh: None,
            list: List::default(),
            saved: None,
            back: Page::Overview,
            request: 0,
        }
    }
}

/// A file picked: its name and text; None when the dialog was closed.
type Picked = Option<(String, String)>;

#[derive(Clone)]
pub enum Msg {
    Go(Page),
    ProfileRead(Result<Option<String>, String>),
    UseOwnCv(bool),
    CvEdit(text_editor::Action),
    JobEdit(text_editor::Action),
    JobUrl(String),
    ChooseFile(Target),
    FileRead(Target, Result<Picked, String>),
    Fetch,
    Fetched(Result<Arc<Fetched>, String>),
    Match,
    /// A step of the match, as it starts.
    Matching(String),
    Matched(Arc<Result<MatchAnswer, String>>),
    ListLoaded {
        request: u64,
        more: bool,
        list: Result<Arc<SavedMatchList>, String>,
    },
    ShowMore,
    SavedLoaded {
        request: u64,
        result: Result<Arc<MatchResult>, String>,
    },
    Filter(&'static str),
    OpenReject(bool),
    Tick(&'static str, bool),
    Note(&'static str, String),
    Reject,
    BringBack,
    Rejected {
        /// The saved match rejected or brought back.
        id: i64,
        undo: bool,
        rejections: Arc<Vec<Rejection>>,
        outcome: Result<RejectOutcome, String>,
    },
    /// For the shell: a web page to open in the browser.
    OpenPage(String),
    /// For the shell: a match was saved or rejected, so the overview is read again.
    Changed,
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    if is_stale(state.request, &msg) || (state.busy && starts_work(&msg)) {
        return Task::none();
    }
    match msg {
        Msg::Go(page) => return go(state, page, runner),
        Msg::ProfileRead(read) => match read {
            // Another CV chosen stays chosen; the profile is there to go back to.
            Ok(profile) => state.cv.profile = Some(profile),
            Err(why) => {
                state.cv.profile = Some(None);
                state.said = Some((Health::Attention, format!("Your profile couldn't be read: {why}")));
            }
        },
        Msg::UseOwnCv(own) => state.cv.own = own,
        Msg::CvEdit(action) => state.cv.text.perform(action),
        Msg::JobEdit(action) => state.job.perform(action),
        Msg::JobUrl(url) => state.job_url = url,
        Msg::ChooseFile(target) => {
            state.busy = true;
            return runner.run(
                |context| async move { pick_file(context).await },
                move |picked| Msg::FileRead(target, picked.and_then(|picked| picked.map_err(|e| e.message))),
            );
        }
        Msg::FileRead(target, picked) => {
            state.busy = false;
            match picked {
                Ok(None) => {}
                Ok(Some((name, text))) => {
                    let content = text_editor::Content::with_text(&text);
                    match target {
                        Target::Cv => {
                            state.cv.text = content;
                            state.cv.file_name = Some(name);
                            state.cv.own = true;
                        }
                        Target::Job => {
                            state.job = content;
                            state.job_file = Some(name);
                        }
                    }
                    state.said = None;
                }
                Err(why) => state.said = Some((Health::Attention, why)),
            }
        }
        Msg::Fetch => {
            let url = state.job_url.trim().to_string();
            if !match_service::is_link(&url) {
                state.said = Some((
                    Health::Attention,
                    "Paste a link starting with http:// or https://.".to_string(),
                ));
                return Task::none();
            }
            state.busy = true;
            state.said = Some((Health::Neutral, "Fetching the job post…".to_string()));
            return runner.run(
                move |context| async move { match_service::fetch_job(&context, &url).await.map(Arc::new) },
                |page| Msg::Fetched(page.and_then(|page| page.map_err(|e| e.message))),
            );
        }
        Msg::Fetched(page) => {
            state.busy = false;
            state.said = Some(match page {
                Ok(page) => {
                    put_fetched(state, &page);
                    (
                        Health::Ready,
                        format!(
                            "Read {} characters from {}. Check the text, then match.",
                            page.text.chars().count(),
                            host_of(&page.url)
                        ),
                    )
                }
                Err(why) => (Health::Attention, why),
            });
        }
        Msg::Match => return start_match(state, runner),
        Msg::Matching(step) => state.said = Some((Health::Neutral, step)),
        Msg::Matched(answer) => return matched(state, &answer),
        Msg::ListLoaded { more, list, .. } => match list {
            Ok(list) => {
                if !more {
                    state.list.rows.clear();
                }
                state.list.rows.extend(list.matches.iter().cloned());
                state.list.next_after = list.next_after.clone();
                state.list.read = Some(Ok(()));
            }
            Err(why) => state.list.read = Some(Err(why)),
        },
        Msg::ShowMore => {
            if let (Page::List(list), Some(after)) = (state.page, state.list.next_after.clone()) {
                return load_list(state, list, after, runner);
            }
        }
        Msg::SavedLoaded { result, .. } => {
            state.saved = Some(result.map(|result| Shown::new(Arc::unwrap_or_clone(result))));
        }
        Msg::Filter(key) => {
            if let Some(shown) = shown_mut(state) {
                shown.filter = key;
            }
        }
        Msg::OpenReject(open) => {
            if let Some(shown) = shown_mut(state) {
                shown.rejecting = open;
                shown.problem = None;
            }
        }
        Msg::Tick(reason, on) => {
            if let Some(shown) = shown_mut(state) {
                shown.ticked.retain(|ticked| *ticked != reason);
                if on {
                    shown.ticked.push(reason);
                }
            }
        }
        Msg::Note(reason, note) => {
            if let Some(shown) = shown_mut(state) {
                let note: String = note.chars().take(reject_reason::NOTE_MAX).collect();
                shown.notes.retain(|(of, _)| *of != reason);
                shown.notes.push((reason, note));
            }
        }
        Msg::Reject => return reject(state, false, runner),
        Msg::BringBack => return reject(state, true, runner),
        Msg::Rejected {
            id,
            undo,
            rejections,
            outcome,
        } => {
            state.busy = false;
            match outcome {
                Ok(RejectOutcome::Done(said)) => {
                    // Every result of this match shows it, whichever page is open now.
                    for shown in results_of(state, id) {
                        shown.result.reject = RejectButton::from(Some(id), (!undo).then_some(rejections.as_slice()));
                        shown.rejecting = false;
                        shown.ticked.clear();
                        shown.notes.clear();
                        shown.problem = None;
                    }
                    state.said = said.map(|said| (Health::Ready, said.to_string()));
                    // The lists have changed.
                    if let Page::List(list) = state.page {
                        return Task::batch([go(state, Page::List(list), runner), Task::done(Msg::Changed)]);
                    }
                    state.list.read = None;
                    return Task::done(Msg::Changed);
                }
                Ok(RejectOutcome::Unfinished(problem)) => {
                    for shown in results_of(state, id) {
                        shown.problem = Some(problem.clone());
                    }
                }
                Err(why) => state.said = Some((Health::Attention, why)),
            }
        }
        // The shell's.
        Msg::OpenPage(_) | Msg::Changed => {}
    }
    Task::none()
}

/// A click that starts work, which waits while other work runs (a disabled button can't send
/// it, but Enter in the link field can), so two answers can't land on the form in either order.
fn starts_work(msg: &Msg) -> bool {
    matches!(
        msg,
        Msg::Fetch | Msg::Match | Msg::ChooseFile(_) | Msg::Reject | Msg::BringBack
    )
}

/// An answer to a read older than the newest.
fn is_stale(request: u64, msg: &Msg) -> bool {
    match msg {
        Msg::ListLoaded { request: asked, .. } | Msg::SavedLoaded { request: asked, .. } => *asked != request,
        _ => false,
    }
}

fn go(state: &mut State, page: Page, runner: &Runner) -> Task<Msg> {
    if !state.busy {
        state.said = None;
    }
    if matches!(page, Page::Saved(_)) && !matches!(state.page, Page::Saved(_)) {
        state.back = state.page;
    }
    state.page = page;
    match page {
        Page::Overview => Task::none(),
        // Read on every visit, so a CV read again in setup is the one matched.
        Page::New => runner.run(
            |context| async move { profile::text_for_user(context.db(), context.user_id).await },
            |read| Msg::ProfileRead(read.and_then(|read| read.map_err(|e| e.to_string()))),
        ),
        Page::List(list) => {
            state.list.empty = LISTS
                .iter()
                .find(|(name, ..)| *name == list)
                .map_or("", |(.., empty)| *empty);
            // Another list: nothing of the last one stays, its cursor least of all.
            state.list.read = None;
            state.list.rows.clear();
            state.list.next_after = None;
            load_list(state, list, String::new(), runner)
        }
        Page::Saved(id) => {
            state.request += 1;
            let request = state.request;
            state.saved = None;
            runner.run(
                move |context| async move { match_service::saved_result(&context, id).await.map(Arc::new) },
                move |result| Msg::SavedLoaded {
                    request,
                    result: result.and_then(|result| result.map_err(|e| e.message)),
                },
            )
        }
    }
}

/// Reads a batch of list after the cursor after ("" for the first one).
fn load_list(state: &mut State, list: &'static str, after: String, runner: &Runner) -> Task<Msg> {
    state.request += 1;
    let request = state.request;
    let more = !after.is_empty();
    runner.run(
        move |context| async move {
            match_service::saved_matches(&context, Some(list), &after)
                .await
                .map(Arc::new)
        },
        move |list| Msg::ListLoaded {
            request,
            more,
            list: list.and_then(|list| list.map_err(|e| e.message)),
        },
    )
}

fn start_match(state: &mut State, runner: &Runner) -> Task<Msg> {
    state.busy = true;
    state.said = Some((Health::Neutral, "Matching…".to_string()));
    let job_text = state.job.text();
    let job_url = state.job_url.clone();
    let cv_text = state.cv.text();
    let read_cv = state.cv.read.clone();
    runner.run_saying(
        move |context, say| async move {
            let ask = MatchAsk {
                job_text: &job_text,
                job_url: &job_url,
                cv_text: &cv_text,
                read_cv: read_cv.as_ref(),
            };
            match_service::match_job(&context, ask, &*say).await
        },
        |step| match step {
            Step::Said(line) => Msg::Matching(line),
            Step::Done(answer) => Msg::Matched(Arc::new(answer.and_then(|answer| answer.map_err(|e| e.message)))),
        },
    )
}

fn matched(state: &mut State, answer: &Result<MatchAnswer, String>) -> Task<Msg> {
    state.busy = false;
    let answer = match answer {
        Ok(answer) => answer,
        Err(why) => {
            state.said = Some((Health::Attention, why.clone()));
            return Task::none();
        }
    };
    if let Some(page) = &answer.fetched {
        put_fetched(state, page);
    }
    if let Some(read) = &answer.read_cv {
        state.cv.read = Some(read.clone());
    }
    state.said = if answer.matched_before {
        Some((
            Health::Neutral,
            "You matched this post with this CV before, so this is the saved match. Change the CV or the post to match again."
                .to_string(),
        ))
    } else {
        answer.save_error.clone().map(|why| (Health::Attention, why))
    };
    state.fresh = Some(Shown::new(answer.result.clone()));
    let saved_now = answer.result.saved_id.is_some() && !answer.matched_before;
    if saved_now {
        state.list.read = None;
        return Task::done(Msg::Changed);
    }
    Task::none()
}

/// The fetched post in the job boxes.
fn put_fetched(state: &mut State, page: &Fetched) {
    state.job_url = page.url.clone();
    state.job = text_editor::Content::with_text(&page.text);
    state.job_file = None;
}

/// "www.jobindex.dk" of "https://www.jobindex.dk/job/1".
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

/// The result on the page shown.
fn shown_mut(state: &mut State) -> Option<&mut Shown> {
    match state.page {
        Page::New => state.fresh.as_mut(),
        Page::Saved(_) => state.saved.as_mut().and_then(|saved| saved.as_mut().ok()),
        Page::Overview | Page::List(_) => None,
    }
}

/// The results on screen of saved match id: the last match's, and the one open on its page.
fn results_of(state: &mut State, id: i64) -> impl Iterator<Item = &mut Shown> {
    let saved = state.saved.as_mut().and_then(|saved| saved.as_mut().ok());
    state
        .fresh
        .as_mut()
        .into_iter()
        .chain(saved)
        .filter(move |shown| shown.result.saved_id == Some(id))
}

fn reject(state: &mut State, undo: bool, runner: &Runner) -> Task<Msg> {
    let Some(shown) = shown_mut(state) else {
        return Task::none();
    };
    let Some(id) = shown.result.saved_id else {
        return Task::none();
    };
    let reasons: Vec<String> = if undo {
        Vec::new()
    } else {
        shown.ticked.iter().map(|reason| reason.to_string()).collect()
    };
    let notes: Vec<(String, String)> = if undo {
        Vec::new()
    } else {
        shown
            .notes
            .iter()
            .map(|(reason, note)| (reason.to_string(), note.clone()))
            .collect()
    };
    let rejections = Arc::new(reject_reason::rows(&reasons, &notes));
    state.busy = true;
    runner.run(
        move |context| async move { match_service::reject(&context, id, &reasons, &notes, undo).await },
        move |outcome| Msg::Rejected {
            id,
            undo,
            rejections: Arc::clone(&rejections),
            outcome: outcome.and_then(|outcome| outcome.map_err(|e| e.message)),
        },
    )
}

/// A CV or job post from a file: a PDF's text (read_pdf) or a text file's; None when the dialog
/// was closed.
async fn pick_file(context: Arc<Context>) -> Result<Picked, ServiceError> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Choose a PDF or text file")
        .add_filter("PDF or text", &FILE_KINDS)
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let name = file.file_name();
    let bytes = file.read().await;
    if bytes.len() > match_service::MAX_PDF_BYTES {
        return Err(ServiceError::new(
            ErrorKind::TooLarge,
            format!("{name} is over {} MB.", match_service::MAX_PDF_BYTES / (1024 * 1024)),
        ));
    }
    let text = if bytes.starts_with(PDF_START) {
        match_service::read_pdf(&context, bytes).await?
    } else {
        String::from_utf8(bytes)
            .map_err(|_| ServiceError::new(ErrorKind::Invalid, format!("{name} is neither a PDF nor a text file.")))?
    };
    Ok(Some((name, text)))
}

// ---------- the pages ----------

pub fn view(state: &State) -> Element<'_, Msg> {
    let mut screen = Column::new().spacing(GAP * 1.5);
    screen = screen.push(back_button(state));
    match state.page {
        // The shell shows the overview.
        Page::Overview => {}
        Page::New => screen = screen.push(new_view(state)),
        Page::List(list) => screen = screen.push(list_view(state, list)),
        Page::Saved(_) => screen = screen.push(saved_view(state)),
    }
    screen.into()
}

fn back_button(state: &State) -> Element<'_, Msg> {
    let (label, to) = match (state.page, state.back) {
        (Page::Saved(_), Page::List(list)) => ("‹ Saved matches", Page::List(list)),
        (Page::Saved(_), Page::New) => ("‹ Match a job", Page::New),
        _ => ("‹ Overview", Page::Overview),
    };
    button(text(label).size(style::BODY_SIZE))
        .padding([4.0, 8.0])
        .style(style::subtle_button(false))
        .on_press(Msg::Go(to))
        .into()
}

/// The step running or what the last click came to.
fn said_bar(state: &State) -> Option<Element<'_, Msg>> {
    state
        .said
        .as_ref()
        .map(|(health, said)| widgets::message_bar(*health, said.as_str()))
}

fn new_view(state: &State) -> Element<'_, Msg> {
    let mut page = Column::new().spacing(GAP * 1.5).push(widgets::page_title(
        "Match a job",
        widgets::note("Paste a link to a job post, its text, or choose a file, and match it with your CV."),
    ));
    page = page
        .push(widgets::section("Job post", job_card(state)))
        .push(widgets::section("CV", cv_card(state)));
    let match_button = button(text("Match").size(style::BODY_SIZE))
        .padding([8.0, 24.0])
        .style(style::primary_button)
        .on_press_maybe((!state.busy).then_some(Msg::Match));
    page = page.push(match_button);
    if let Some(bar) = said_bar(state) {
        page = page.push(bar);
    }
    if let Some(shown) = &state.fresh {
        page = page.push(result_view(shown, state.busy));
    }
    page.into()
}

fn job_card(state: &State) -> Element<'_, Msg> {
    let link = row![
        text_input("https://…", &state.job_url)
            .on_input(Msg::JobUrl)
            .on_submit(Msg::Fetch)
            .size(style::BODY_SIZE)
            .padding(8)
            .style(style::text_field)
            .width(Fill),
        button(text("Fetch").size(style::BODY_SIZE))
            .style(style::standard_button)
            .on_press_maybe((!state.busy).then_some(Msg::Fetch)),
    ]
    .spacing(8)
    .align_y(Center);
    let mut body = column![
        widgets::field("Link to the post", link.into(), None),
        widgets::field(
            "The post's text",
            text_editor(&state.job)
                .placeholder("Paste the job post here, or a link to it")
                .on_action(Msg::JobEdit)
                .size(style::BODY_SIZE)
                .padding(8)
                .height(JOB_HEIGHT)
                .style(style::text_area)
                .into(),
            None,
        ),
    ]
    .spacing(GAP);
    body = body.push(file_row(state, Target::Job, state.job_file.as_deref()));
    container(body).padding(CARD_PADDING).width(Fill).into()
}

fn cv_card(state: &State) -> Element<'_, Msg> {
    let cv = &state.cv;
    let mut body = Column::new().spacing(GAP);
    match &cv.profile {
        None => body = body.push(widgets::caption("Reading your profile…")),
        Some(Some(_)) if !cv.own => {
            body = body.push(
                row![
                    style::mark(Health::Ready),
                    column![
                        text("Your profile from setup")
                            .font(style::semibold())
                            .size(style::BODY_SIZE),
                        widgets::caption(read_cv_line(cv)),
                    ]
                    .spacing(2)
                    .width(Fill),
                    button(text("Use another CV").size(style::BODY_SIZE))
                        .style(style::standard_button)
                        .on_press_maybe((!state.busy).then_some(Msg::UseOwnCv(true))),
                ]
                .spacing(14)
                .align_y(Center),
            );
        }
        Some(profile) => {
            if profile.is_some() {
                body = body.push(
                    row![
                        text("Matching with the CV below instead of your profile.")
                            .size(style::CAPTION_SIZE)
                            .style(style::secondary)
                            .width(Fill),
                        button(text("Use my profile").size(style::BODY_SIZE))
                            .style(style::standard_button)
                            .on_press_maybe((!state.busy).then_some(Msg::UseOwnCv(false))),
                    ]
                    .spacing(14)
                    .align_y(Center),
                );
            } else {
                body = body.push(widgets::caption(
                    "There's no profile from setup yet, so paste a CV or choose its file.",
                ));
            }
            body = body
                .push(
                    text_editor(&cv.text)
                        .placeholder("Paste your CV here")
                        .on_action(Msg::CvEdit)
                        .size(style::BODY_SIZE)
                        .padding(8)
                        .height(CV_HEIGHT)
                        .style(style::text_area),
                )
                .push(file_row(state, Target::Cv, cv.file_name.as_deref()));
            if !cv.uses_profile() && cv.read.as_ref().is_some_and(|read| read.text == cv.text().trim()) {
                body = body.push(widgets::caption(read_cv_line(cv)));
            }
        }
    }
    container(body).padding(CARD_PADDING).width(Fill).into()
}

/// What was read in the CV, or when it is read.
fn read_cv_line(cv: &Cv) -> String {
    match cv.read.as_ref().filter(|read| read.text == cv.text().trim()) {
        Some(read) => {
            let profile = &read.profile;
            let name = if profile.name.is_empty() {
                "CV"
            } else {
                profile.name.as_str()
            };
            format!(
                "{name}: {} years, {} skills.",
                profile.experience_years,
                profile.skill.len()
            )
        }
        None => "It's read when you match.".to_string(),
    }
}

/// Choose file, and the file chosen.
fn file_row<'a>(state: &'a State, target: Target, file_name: Option<&'a str>) -> Element<'a, Msg> {
    let mut line = row![
        button(text("Choose file…").size(style::BODY_SIZE))
            .style(style::standard_button)
            .on_press_maybe((!state.busy).then_some(Msg::ChooseFile(target)))
    ]
    .spacing(10)
    .align_y(Center);
    line = line.push(widgets::caption(match file_name {
        Some(name) => format!("From {name}"),
        None => "A PDF or a text file.".to_string(),
    }));
    line.into()
}

fn list_view<'a>(state: &'a State, current: &'static str) -> Element<'a, Msg> {
    let tabs = Row::with_children(LISTS.iter().map(|(list, label, _)| {
        button(text(*label).size(style::BODY_SIZE))
            .padding([6.0, 12.0])
            .style(style::subtle_button(*list == current))
            .on_press(Msg::Go(Page::List(list)))
            .into()
    }))
    .spacing(4)
    .wrap();
    let mut page = Column::new()
        .spacing(GAP * 1.5)
        .push(widgets::page_title(
            "Saved matches",
            widgets::note("Best first. Open one to see its verdicts, or to say you're not interested."),
        ))
        .push(tabs);
    if let Some(bar) = said_bar(state) {
        page = page.push(bar);
    }
    let body: Element<'_, Msg> = match &state.list.read {
        None => widgets::note("Reading the saved matches…"),
        Some(Err(why)) => widgets::message_bar(Health::Attention, why.as_str()),
        Some(Ok(())) if state.list.rows.is_empty() => {
            widgets::card(container(widgets::note(state.list.empty)).padding(CARD_PADDING).into())
        }
        Some(Ok(())) => widgets::card(widgets::rows(state.list.rows.iter().map(saved_row))),
    };
    page = page.push(body);
    if state.list.next_after.is_some() && matches!(state.list.read, Some(Ok(()))) {
        page = page.push(
            button(text("Show more").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(Msg::ShowMore),
        );
    }
    page.into()
}

/// A saved match in a list, opening its result.
fn saved_row(saved: &SavedMatchRow) -> Element<'_, Msg> {
    let mut about = column![
        text(saved.title.as_str())
            .font(style::semibold())
            .size(style::BODY_SIZE),
        widgets::caption(saved.about.as_str()),
    ]
    .spacing(2)
    .width(Fill);
    if !saved.done.is_empty() {
        about = about.push(widgets::caption(saved.done.as_str()));
    }
    button(
        row![
            about,
            widgets::caption(saved.saved_on.as_str()),
            text("›").size(style::SECTION_SIZE).style(style::secondary),
        ]
        .spacing(14)
        .align_y(Center),
    )
    .padding(ROW_PADDING)
    .width(Fill)
    .style(style::row_button)
    .on_press(Msg::Go(Page::Saved(saved.id)))
    .into()
}

fn saved_view(state: &State) -> Element<'_, Msg> {
    let mut page = Column::new().spacing(GAP * 1.5);
    if let Some(bar) = said_bar(state) {
        page = page.push(bar);
    }
    page.push(match &state.saved {
        None => widgets::note("Reading the match…"),
        Some(Err(why)) => widgets::message_bar(Health::Attention, why.as_str()),
        Some(Ok(shown)) => result_view(shown, state.busy),
    })
    .into()
}

// ---------- a result ----------

fn result_view(shown: &Shown, busy: bool) -> Element<'_, Msg> {
    let result = &shown.result;
    let mut view = Column::new().spacing(GAP * 1.5).push(score_card(result));
    if let Some(reject) = &result.reject {
        view = view.push(reject_card(shown, reject, busy));
    }
    if let Some(location) = &result.location {
        let mut lines = column![text(location.heading).font(style::semibold()).size(style::BODY_SIZE)].spacing(4);
        for line in &location.lines {
            lines = lines.push(text(join_reason(&line.text, &line.reason)).size(style::BODY_SIZE));
        }
        lines = lines.push(widgets::caption(location.note));
        view = view.push(
            container(row![style::mark(Health::Attention), lines.width(Fill)].spacing(12))
                .padding([12.0, CARD_PADDING])
                .width(Fill)
                .style(style::warning_bar),
        );
    }
    if let Some(apply) = &result.apply {
        let mut lines = column![text(apply.heading).font(style::semibold()).size(style::BODY_SIZE)].spacing(4);
        if !apply.note.is_empty() {
            lines = lines.push(widgets::caption(apply.note.as_str()));
        }
        for (label, value) in [
            ("Send it to", &apply.email),
            ("Subject line:", &apply.subject),
            ("Include:", &apply.keyword),
        ] {
            if !value.is_empty() {
                lines = lines.push(text(format!("{label} {value}")).size(style::BODY_SIZE));
            }
        }
        if !apply.asks.is_empty() {
            lines = lines.push(text(apply.asks.as_str()).size(style::BODY_SIZE));
        }
        for line in &apply.lines {
            lines = lines.push(widgets::caption(format!("“{line}”")));
        }
        let health = if apply.trap { Health::Attention } else { Health::Neutral };
        view = view.push(
            container(row![style::mark(health), lines.width(Fill)].spacing(12))
                .padding([12.0, CARD_PADDING])
                .width(Fill)
                .style(if apply.trap { style::warning_bar } else { style::card }),
        );
    }
    if !result.filters.is_empty() {
        let filters = Row::with_children(result.filters.iter().map(|filter| {
            button(text(format!("{} {}", filter.label, filter.count)).size(style::BODY_SIZE))
                .padding([6.0, 12.0])
                .style(style::subtle_button(filter.key == shown.filter))
                .on_press(Msg::Filter(filter.key))
                .into()
        }))
        .spacing(4)
        .wrap();
        view = view.push(filters);
        for section in &result.sections {
            if let Some(card) = section_card(section, shown.filter) {
                view = view.push(card);
            }
        }
    }
    if !result.soft_skills.is_empty() {
        let chips = Row::with_children(result.soft_skills.iter().map(|skill| {
            container(text(skill.as_str()).size(style::CAPTION_SIZE))
                .padding([2.0, 10.0])
                .style(style::chip(Health::Neutral))
                .into()
        }))
        .spacing(6)
        .wrap();
        view = view.push(
            column![
                row![widgets::section_title("Soft skills"), widgets::caption("not scored")]
                    .spacing(8)
                    .align_y(Center),
                chips,
            ]
            .spacing(10),
        );
    }
    if !result.laya.text.is_empty() {
        view = view.push(if result.laya.failed {
            widgets::caption(result.laya.text.as_str())
        } else {
            text(result.laya.text.as_str()).size(style::BODY_SIZE).into()
        });
    }
    if let Some(id) = result.saved_id {
        view = view.push(widgets::section(
            "Application and résumé",
            container(
                row![
                    text(
                        "Writing the application and the résumé, and logging it on Jobnet, are still on the match's web page."
                    )
                    .size(style::CAPTION_SIZE)
                    .style(style::secondary)
                    .width(Fill),
                    button(text("Open in the browser").size(style::BODY_SIZE))
                        .style(style::standard_button)
                        .on_press(Msg::OpenPage(format!("match/{id}"))),
                ]
                .spacing(14)
                .align_y(Center),
            )
            .padding(CARD_PADDING)
            .into(),
        ));
    }
    view.into()
}

/// The score, the job and how the score is made up.
fn score_card(result: &MatchResult) -> Element<'_, Msg> {
    let score = &result.score;
    let mut under = row![
        container(text(score.tag_name.as_str()).size(style::CAPTION_SIZE))
            .padding([2.0, 10.0])
            .style(style::chip(tag_health(&score.tag))),
    ]
    .spacing(10)
    .align_y(Center);
    if let Some(day) = result.saved_at.get(..10) {
        under = under.push(widgets::caption(format!("saved {day}")));
    }
    widgets::card(
        container(
            row![
                text(format!("{}%", score.percent))
                    .font(style::semibold())
                    .size(SCORE_SIZE),
                column![
                    text(if result.heading.is_empty() {
                        "Untitled"
                    } else {
                        result.heading.as_str()
                    })
                    .font(style::semibold())
                    .size(style::SECTION_SIZE),
                    widgets::caption(result.parts.as_str()),
                    under,
                ]
                .spacing(4)
                .width(Fill),
            ]
            .spacing(20)
            .align_y(Center),
        )
        .padding(CARD_PADDING)
        .into(),
    )
}

/// A score tag's colours: top and good are good news, weak is not.
fn tag_health(tag: &str) -> Health {
    match tag {
        "top" | "good" => Health::Ready,
        "poor" => Health::Attention,
        _ => Health::Neutral,
    }
}

/// Not interested, its form, or the rejected match's Bring back.
fn reject_card<'a>(shown: &'a Shown, reject: &'a RejectButton, busy: bool) -> Element<'a, Msg> {
    if reject.rejected {
        return widgets::card(
            container(
                row![
                    text(reject.note.as_str())
                        .size(style::CAPTION_SIZE)
                        .style(style::secondary)
                        .width(Fill),
                    button(text(reject.submit).size(style::BODY_SIZE))
                        .style(style::standard_button)
                        .on_press_maybe((!busy).then_some(Msg::BringBack)),
                ]
                .spacing(14)
                .align_y(Center),
            )
            .padding(ROW_PADDING)
            .into(),
        );
    }
    if !shown.rejecting {
        return button(text(reject.open).size(style::BODY_SIZE))
            .style(style::standard_button)
            .on_press_maybe((!busy).then_some(Msg::OpenReject(true)))
            .into();
    }
    let mut form = column![
        text("Why not? Tick all that apply")
            .font(style::semibold())
            .size(style::BODY_SIZE)
    ]
    .spacing(8);
    for reason in &reject.reasons {
        let ticked = shown.ticked.contains(&reason.value);
        let value = reason.value;
        form = form.push(
            row![
                checkbox(ticked)
                    .label(reason.label)
                    .text_size(style::BODY_SIZE)
                    .style(style::check_box)
                    .on_toggle(move |on| Msg::Tick(value, on))
                    .width(REASON_WIDTH),
                text_input(reason.placeholder, shown.note(value))
                    .on_input(move |note| Msg::Note(value, note))
                    .size(style::BODY_SIZE)
                    .padding(6)
                    .style(style::text_field)
                    .width(Fill),
            ]
            .spacing(12)
            .align_y(Center),
        );
    }
    if let Some(problem) = &shown.problem {
        form = form.push(
            text(problem.as_str())
                .size(style::CAPTION_SIZE)
                .style(style::health_text(Health::Attention)),
        );
    }
    form = form.push(
        row![
            button(text(reject_button::REJECT).size(style::BODY_SIZE))
                .style(style::danger_button)
                .on_press_maybe((!busy).then_some(Msg::Reject)),
            button(text("Cancel").size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(Msg::OpenReject(false)),
        ]
        .spacing(8),
    );
    widgets::card(container(form).padding(CARD_PADDING).into())
}

/// The width of a reason's checkbox, so the notes line up.
const REASON_WIDTH: f32 = 200.0;

/// A group of requirements with the rows the filter shows; None when it shows none.
fn section_card<'a>(section: &'a Section, filter: &'static str) -> Option<Element<'a, Msg>> {
    let rows: Vec<&VerdictRow> = section
        .rows
        .iter()
        .filter(|row| filter == ALL || row.verdict == filter)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let mut heading = row![widgets::section_title(section.heading.as_str())]
        .spacing(8)
        .align_y(Center);
    if !section.note.is_empty() {
        heading = heading.push(widgets::caption(section.note.as_str()));
    }
    Some(
        column![
            heading,
            widgets::card(widgets::rows(rows.into_iter().map(|row| verdict_view(row, 0.0))))
        ]
        .spacing(10)
        .into(),
    )
}

/// The verdict chip's column, so the requirements line up.
const VERDICT_WIDTH: f32 = 72.0;
/// How far an alternative is set in under its requirement.
const ALTERNATIVE_INDENT: f32 = 24.0;

/// A requirement: its verdict, text, why, the CV's words and what decided it; its alternatives
/// under it.
fn verdict_view(row: &VerdictRow, indent: f32) -> Element<'_, Msg> {
    let mut body = column![text(row.text.as_str()).font(style::semibold()).size(style::BODY_SIZE)].spacing(2);
    if !row.reason.is_empty() {
        body = body.push(text(row.reason.as_str()).size(style::CAPTION_SIZE));
    }
    if !row.evidence.is_empty() {
        body = body.push(widgets::caption(format!("“{}”", row.evidence)));
    }
    if !row.decided_by.is_empty() {
        body = body.push(widgets::caption(row.decided_by.as_str()));
    }
    let line = iced::widget::row![
        container(
            container(text(row.verdict).size(style::CAPTION_SIZE))
                .padding([2.0, 10.0])
                .style(style::chip(verdict_health(row.verdict)))
        )
        .width(VERDICT_WIDTH),
        body.width(Fill),
    ]
    .spacing(12)
    .padding(iced::Padding::from(ROW_PADDING).left(CARD_PADDING + indent));
    if row.alternatives.is_empty() {
        return line.into();
    }
    Column::with_children(
        std::iter::once(line.into()).chain(
            row.alternatives
                .iter()
                .map(|alternative| verdict_view(alternative, indent + ALTERNATIVE_INDENT)),
        ),
    )
    .into()
}

/// A verdict's colours.
fn verdict_health(verdict: &str) -> Health {
    match verdict {
        "met" => Health::Ready,
        "missing" => Health::Attention,
        _ => Health::Neutral,
    }
}

/// "Located near Copenhagen · Not named in the CV."
fn join_reason(text: &str, reason: &str) -> String {
    if reason.is_empty() {
        text.to_string()
    } else {
        format!("{text} · {reason}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_to_an_older_read_is_dropped() {
        let old = Msg::SavedLoaded {
            request: 1,
            result: Err("old".to_string()),
        };
        assert!(is_stale(2, &old));
        assert!(!is_stale(1, &old));
        assert!(!is_stale(2, &Msg::Match));
    }

    #[test]
    fn work_waits_for_the_work_running() {
        assert!(starts_work(&Msg::Fetch));
        assert!(starts_work(&Msg::Match));
        assert!(!starts_work(&Msg::JobUrl(String::new())));
        assert!(!starts_work(&Msg::Matching(String::new())));
    }

    #[test]
    fn the_host_is_named_without_its_path() {
        assert_eq!(host_of("https://www.jobindex.dk/job/1?x=2"), "www.jobindex.dk");
        assert_eq!(host_of("https://jobnet.dk"), "jobnet.dk");
    }

    #[test]
    fn the_cv_matched_is_the_profile_until_another_is_chosen() {
        let mut cv = Cv {
            profile: Some(Some("Profile text".to_string())),
            text: text_editor::Content::with_text("Pasted CV"),
            ..Cv::default()
        };
        assert_eq!(cv.text(), "Profile text");
        cv.own = true;
        assert!(cv.text().starts_with("Pasted CV"));
        let without = Cv {
            profile: Some(None),
            text: text_editor::Content::with_text("Pasted CV"),
            ..Cv::default()
        };
        assert!(without.text().starts_with("Pasted CV"));
    }
}
