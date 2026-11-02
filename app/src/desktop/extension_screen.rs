//! The Jobnet extension screen: the joblog_extension page in the window. Where the app keeps the
//! extension it ships (extension), how to load it in Edge or Chrome, how to use it, and what it
//! sends where.

use iced::widget::{Column, button, column, container, row, text};
use iced::{Center, Element, Fill, Task};

use super::GAP;
use super::style;
use super::widgets::{self, CARD_PADDING};
use crate::context::Context;
use crate::dashboard::Health;
use crate::extension;

#[derive(Default)]
pub struct State {
    said: Option<(Health, String)>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// Copies the folder and opens Edge's extensions page.
    CopyAndOpen(String),
}

pub fn update(state: &mut State, msg: Msg) -> Task<Msg> {
    match msg {
        Msg::CopyAndOpen(folder) => {
            state.said = Some(match extension::open_extensions_page() {
                Ok(()) => (
                    Health::Ready,
                    "The folder is copied and Edge shows its extensions: paste the folder after Load unpacked."
                        .to_string(),
                ),
                Err(e) => {
                    crate::log_line!("joblog_extension: open Edge: {e}");
                    (
                        Health::Attention,
                        format!(
                            "The folder is copied, but Edge couldn't be opened: {e} Open {} yourself.",
                            extension::EDGE_EXTENSIONS_PAGE
                        ),
                    )
                }
            });
            iced::clipboard::write(folder)
        }
    }
}

/// Clears what the last click said, for a new visit.
pub fn shown(state: &mut State) {
    state.said = None;
}

pub fn view<'a>(state: &'a State, context: &'a Context) -> Element<'a, Msg> {
    let title = widgets::page_title(
        "Joblog autofill",
        widgets::note(
            "A browser extension that fills in Jobnet's \"Opret joblog\" form from a match you saved here: job title, company, address, deadline, contact, the day you applied and your notes. The jobs you have applied for but not logged on Jobnet yet come first. It never presses Gem: you check the form and save it yourself, and the app then knows that job is on Jobnet.",
        ),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    screen
        .push(widgets::section("Set it up once, in Edge or Chrome", set_up(context)))
        .push(widgets::section("Use it", padded(numbered(&USE_STEPS, USE_NOTE))))
        .push(widgets::section("What it sends where", padded(bullets(&SENDS))))
        .into()
}

/// Setting up: the folder, the button, and the steps; or why there is no folder.
fn set_up(context: &Context) -> Element<'_, Msg> {
    let body: Element<'_, Msg> = match &context.extension_folder {
        Ok(folder) => {
            let folder = folder.display().to_string();
            column![
                text(format!(
                    "The app keeps the extension (version {}) in this folder:",
                    extension::version()
                ))
                .size(style::BODY_SIZE),
                text(folder.clone()).font(style::semibold()).size(style::BODY_SIZE),
                button(text("Copy the folder and open Edge's extensions").size(style::BODY_SIZE))
                    .style(style::primary_button)
                    .on_press(Msg::CopyAndOpen(folder)),
                numbered(&SET_UP_STEPS, SET_UP_NOTE),
                widgets::caption(FIREFOX),
            ]
            .spacing(10)
            .into()
        }
        Err(why) => column![
            widgets::errors_line(format!(
                "The app couldn't put the extension in a folder for your browser. {why}"
            )),
            widgets::caption(FIREFOX),
        ]
        .spacing(10)
        .into(),
    };
    padded(body)
}

fn padded(body: Element<'_, Msg>) -> Element<'_, Msg> {
    container(body).padding(CARD_PADDING).width(Fill).into()
}

/// Steps as "1. …", with a note under them.
fn numbered<'a>(steps: &'a [&'a str], note: &'a str) -> Element<'a, Msg> {
    let mut list = Column::new().spacing(6);
    for (index, line) in steps.iter().enumerate() {
        list = list.push(
            row![
                text(format!("{}.", index + 1)).size(style::BODY_SIZE).width(20),
                text(*line).size(style::BODY_SIZE).width(Fill),
            ]
            .align_y(Center),
        );
    }
    list.push(widgets::caption(note)).into()
}

fn bullets<'a>(lines: &'a [&'a str]) -> Element<'a, Msg> {
    Column::with_children(lines.iter().map(|line| {
        row![
            text("•").size(style::BODY_SIZE).width(20),
            text(*line).size(style::BODY_SIZE).width(Fill),
        ]
        .into()
    }))
    .spacing(6)
    .into()
}

const SET_UP_STEPS: [&str; 3] = [
    "Press the button above. It copies the folder and opens edge://extensions in Edge. In Chrome, copy the folder and open chrome://extensions yourself.",
    "Turn on Developer mode.",
    "Press \"Load unpacked\", paste the folder into the Folder box and press Select Folder.",
];
const SET_UP_NOTE: &str = "The app writes the newest version into the folder each time it starts. After an update, press Reload on \"Joblog autofill, by JobFinder\" on the extensions page.";
const FIREFOX: &str = "Firefox gives an extension a new ID on every install, which the app can't recognise, so it only works in Edge and Chrome.";
const USE_STEPS: [&str; 3] = [
    "Keep JobFinder running, and save a match on the Match screen.",
    "On Jobnet, open Joblog and \"Opret joblog\". A \"Fill from JobFinder\" box shows in the corner.",
    "Pick the match, press \"Fill in the form\", check what it filled, then press Gem.",
];
const USE_NOTE: &str = "Jobnet only keeps a company or postcode picked from its own suggestions, so the extension picks the closest one and says when it differs from your match. A company Jobnet doesn't know keeps its name as typed.";
const SENDS: [&str; 3] = [
    "It reads your saved matches from this app (127.0.0.1) and fills them into the form in your own Jobnet tab. Jobnet receives them only when you press Gem.",
    "When you save a form it filled in, it tells this app that the match is on Jobnet, with the status you gave it.",
    "When a post names no postcode and town, it looks the company's name up on cvrapi.dk for its registered address. Only the name is sent.",
];
