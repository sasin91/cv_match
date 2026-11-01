//! The Settings screen: where the app's data is, and the settings pages. Until each has its own
//! screen here (the plan's next step), its button opens the web page for it in the browser.

use iced::widget::{Column, button, column, row, text};
use iced::{Center, Element};

use super::{GAP, Message};
use crate::context::Context;

/// A heading's size.
const HEADING_SIZE: f32 = 20.0;
/// A setting's note.
const NOTE_SIZE: f32 = 13.0;

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
        "What the job scan searches for, and its found posts.",
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
    let pages = Column::with_children(PAGES.iter().map(|(label, note, path)| {
        row![
            button(*label)
                .style(button::secondary)
                .on_press(Message::OpenPage(path.to_string())),
            text(*note).size(NOTE_SIZE),
        ]
        .spacing(GAP)
        .align_y(Center)
        .into()
    }))
    .spacing(8);
    let database = if context.database.path.is_empty() {
        "in memory".to_string()
    } else {
        context.database.path.clone()
    };
    column![
        text("Settings").size(HEADING_SIZE),
        text("These open in your browser until they have their place here.").size(NOTE_SIZE),
        pages,
        text("Your data").size(HEADING_SIZE),
        text(format!("Database: {database}")).size(NOTE_SIZE),
    ]
    .spacing(GAP)
    .into()
}
