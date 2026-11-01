//! The Settings screen: the settings pages and where the app's data is. Until each page has its
//! own screen here (the plan's next step), its row opens the web page in the browser.

use iced::widget::{Column, button, column, container, row, space, text};
use iced::{Center, Element, Fill};

use super::style;
use super::{GAP, Message};
use crate::context::Context;

/// The padding inside a card.
const CARD_PADDING: f32 = 16.0;

/// The settings pages: label, what it sets, and its path on the server.
const PAGES: [(&str, &str, &str); 5] = [
    (
        "Language models",
        "The API keys, and what each model is used for.",
        "account",
    ),
    (
        "Local model",
        "The app's own llama.cpp model: download it and pick one.",
        "llama_setup",
    ),
    (
        "Job searches",
        "What the job scan searches for, and the posts it found.",
        "job_scan",
    ),
    (
        "Setup",
        "Your CV, the AI model and the job scan's schedule, step by step.",
        "setup",
    ),
    ("Jobnet extension", "How to load it in Edge.", "joblog_extension"),
];

pub fn view(context: &Context) -> Element<'_, Message> {
    let mut pages = Column::new();
    for (index, (label, note, path)) in PAGES.iter().enumerate() {
        if index > 0 {
            pages = pages.push(container(space()).width(Fill).height(1).style(style::divider));
        }
        pages = pages.push(page_row(label, note, path));
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
            text("These open in your browser until they have their place here.")
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

/// A settings page: its name and what it sets, opening it.
fn page_row(label: &'static str, note: &'static str, path: &'static str) -> Element<'static, Message> {
    button(
        row![
            column![
                text(label).font(style::semibold()).size(style::BODY_SIZE),
                text(note).size(style::CAPTION_SIZE).style(style::secondary),
            ]
            .spacing(2)
            .width(Fill),
            text("↗").size(style::BODY_SIZE).style(style::secondary),
        ]
        .align_y(Center),
    )
    .padding([12.0, CARD_PADDING])
    .width(Fill)
    .style(style::row_button)
    .on_press(Message::OpenPage(path.to_string()))
    .into()
}
