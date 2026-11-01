//! The Settings screen: the settings pages and where the app's data is. A page with a screen
//! here opens it; the others (Setup, the Jobnet extension) still open the web page in the browser.

use iced::widget::{Column, button, column, container, row, space, text};
use iced::{Center, Element, Fill};

use super::style;
use super::{GAP, Message, Screen};
use crate::context::Context;

/// The padding inside a card.
const CARD_PADDING: f32 = 16.0;

/// Where a settings row goes: a screen of the window, or a web page (its path on the server).
#[derive(Clone, Copy)]
enum Target {
    Screen(Screen),
    Page(&'static str),
}

use Target::Page;

/// The settings pages: label, what it sets, and where it is.
const PAGES: [(&str, &str, Target); 5] = [
    (
        "Language models",
        "The API keys, and what each model is used for.",
        Target::Screen(Screen::LanguageModels),
    ),
    (
        "Local model",
        "The app's own llama.cpp model: download it and pick one.",
        Target::Screen(Screen::LocalModel),
    ),
    (
        "Job searches",
        "What the job scan searches for, and the posts it found.",
        Target::Screen(Screen::JobSearches),
    ),
    (
        "Setup",
        "Your CV, the AI model and the job scan's schedule, step by step.",
        Page("setup"),
    ),
    ("Jobnet extension", "How to load it in Edge.", Page("joblog_extension")),
];

pub fn view(context: &Context) -> Element<'_, Message> {
    let mut pages = Column::new();
    for (index, (label, note, target)) in PAGES.iter().enumerate() {
        if index > 0 {
            pages = pages.push(container(space()).width(Fill).height(1).style(style::divider));
        }
        pages = pages.push(page_row(label, note, *target));
    }
    let database = if context.database.path.is_empty() {
        "In memory".to_string()
    } else {
        context.database.path.clone()
    };
    let data = column![
        text("Database").font(style::semibold()).size(style::BODY_SIZE),
        text(database).size(style::CAPTION_SIZE).style(style::secondary),
    ]
    .spacing(2)
    .padding([12.0, CARD_PADDING]);
    column![
        column![
            text("Settings").font(style::semibold()).size(style::TITLE_SIZE),
            text("Those marked ↗ open in your browser until they have their place here.")
                .size(style::BODY_SIZE)
                .style(style::secondary),
        ]
        .spacing(4),
        container(pages).width(Fill).style(style::card),
        text("Your data").font(style::semibold()).size(style::SECTION_SIZE),
        container(data).width(Fill).style(style::card),
    ]
    .spacing(GAP * 1.5)
    .into()
}

/// A settings page: its name and what it sets, opening its screen, or its web page (marked ↗).
fn page_row(label: &'static str, note: &'static str, target: Target) -> Element<'static, Message> {
    let (mark, opens) = match target {
        Target::Screen(screen) => ("›", Message::Show(screen)),
        Page(path) => ("↗", Message::OpenPage(path.to_string())),
    };
    button(
        row![
            column![
                text(label).font(style::semibold()).size(style::BODY_SIZE),
                text(note).size(style::CAPTION_SIZE).style(style::secondary),
            ]
            .spacing(2)
            .width(Fill),
            text(mark).size(style::BODY_SIZE).style(style::secondary),
        ]
        .align_y(Center),
    )
    .padding([12.0, CARD_PADDING])
    .width(Fill)
    .style(style::row_button)
    .on_press(opens)
    .into()
}
