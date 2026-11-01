//! The Match screen: the dashboard's status rows and the glance at the saved matches, as the
//! match page shows them (dashboard.rs). Matching itself is still the web page's, so its button
//! opens it in the browser.

use iced::widget::{Column, button, column, container, row, text};
use iced::{Element, Fill, Font, font};

use super::{GAP, Message};
use crate::dashboard::{Health, StatusRow};
use crate::matches::saved_match_row::SavedMatchRow;

/// A heading's size.
const HEADING_SIZE: f32 = 20.0;
/// The width a status row's mark takes, the same for each mark, so the names line up.
const MARK_WIDTH: f32 = 14.0;
/// A row's smaller lines.
const NOTE_SIZE: f32 = 13.0;

/// A row's name, in the window's font made bold.
fn bold() -> Font {
    Font {
        weight: font::Weight::Bold,
        ..super::default_font()
    }
}

pub fn view(overview: Option<&Result<super::Overview, String>>) -> Element<'_, Message> {
    let overview = match overview {
        None => return text("Reading the status…").into(),
        Some(Err(why)) => {
            return text(format!("The status couldn't be read: {why}"))
                .style(text::danger)
                .into();
        }
        Some(Ok(overview)) => overview,
    };
    let dashboard = &overview.dashboard;
    let mut screen = Column::new().spacing(GAP);
    if dashboard.ai_missing {
        screen = screen.push(
            container(text(
                "No AI model is set up, so matches use the free keyword match and nothing is written for you.",
            ))
            .padding(10)
            .width(Fill)
            .style(container::warning),
        );
    }
    let headline = if dashboard.all_ready {
        text(dashboard.headline.as_str()).style(text::success)
    } else {
        text(dashboard.headline.as_str())
    };
    screen = screen
        .push(
            row![
                headline.size(HEADING_SIZE),
                button("Match a job").on_press(open("match"))
            ]
            .spacing(GAP),
        )
        .push(Column::with_children(dashboard.rows.iter().map(status_row)).spacing(GAP));

    let findings = &overview.findings;
    let counts = row(findings.counts.iter().map(|count| {
        button(text(format!("{} {}", count.label, count.count)))
            .style(button::secondary)
            .on_press(open(&count.href))
            .into()
    }))
    .spacing(6);
    screen = screen.push(text("Your matches").size(HEADING_SIZE)).push(counts);
    if findings.best.is_empty() {
        screen = screen.push(text("No match on the shortlist yet."));
    } else {
        screen = screen.push(text("Best on the shortlist").size(NOTE_SIZE));
        screen = screen.push(Column::with_children(findings.best.iter().map(best_row)).spacing(6));
    }
    screen.into()
}

/// The browser opening path.
fn open(path: &str) -> Message {
    Message::OpenPage(path.to_string())
}

/// One thing the app runs on: a mark for how it stands, its name and text, and its buttons.
fn status_row(status: &StatusRow) -> Element<'_, Message> {
    let mark = text(mark(status.health)).width(MARK_WIDTH).style(match status.health {
        Health::Ready => text::success,
        Health::Attention => text::warning,
        Health::Neutral => text::secondary,
    });
    let mut buttons = row![].spacing(6);
    if let Some(action) = &status.action {
        buttons = buttons.push(button(action.label).on_press(Message::Do(action.work)));
    }
    if let Some(fix) = &status.fix {
        buttons = buttons.push(button(fix.label).style(button::secondary).on_press(open(fix.href)));
    }
    row![
        mark,
        column![
            text(status.name).font(bold()),
            text(status.text.as_str()).size(NOTE_SIZE),
            buttons
        ]
        .spacing(4)
    ]
    .spacing(8)
    .into()
}

/// The row's mark, with its meaning in the shape too (not only the colour).
fn mark(health: Health) -> &'static str {
    match health {
        Health::Ready => "✔",
        Health::Attention => "!",
        Health::Neutral => "•",
    }
}

/// A saved match on the shortlist, opening its page.
fn best_row(saved: &SavedMatchRow) -> Element<'_, Message> {
    button(column![
        text(saved.title.as_str()),
        text(format!("{} · saved {}", saved.about, saved.saved_on)).size(NOTE_SIZE),
    ])
    .style(button::text)
    .on_press(open(&format!("match/{}", saved.id)))
    .into()
}
