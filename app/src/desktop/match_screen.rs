//! The Match screen: the dashboard's status rows and the glance at the saved matches, as the
//! match page shows them (dashboard.rs). Matching itself is still the web page's, so its button
//! opens it in the browser.

use iced::widget::{Column, button, column, container, row, text};
use iced::{Center, Element, Fill};

use super::style;
use super::widgets::{CARD_PADDING, rows, section, section_title};
use super::{GAP, Message};
use crate::dashboard::{Health, ListCount, StatusRow};
use crate::matches::saved_match_row::SavedMatchRow;

/// A stat tile's height, the same for each, so a label that wraps doesn't make one taller.
const TILE_HEIGHT: f32 = 76.0;
/// Space between a status row's parts.
const ROW_GAP: f32 = 14.0;

pub fn view(overview: Option<&Result<super::Overview, String>>) -> Element<'_, Message> {
    let overview = match overview {
        None => return heading("Overview", text("Reading the status…").style(style::secondary).into()),
        Some(Err(why)) => {
            return heading(
                "Overview",
                text(format!("The status couldn't be read: {why}"))
                    .style(style::health_text(Health::Attention))
                    .into(),
            );
        }
        Some(Ok(overview)) => overview,
    };
    let dashboard = &overview.dashboard;
    let headline = text(dashboard.headline.as_str())
        .size(style::BODY_SIZE)
        .style(style::health_text(if dashboard.all_ready {
            Health::Ready
        } else {
            Health::Attention
        }));
    let mut screen = Column::new()
        .spacing(GAP * 1.5)
        .push(heading("Overview", headline.into()));
    if dashboard.ai_missing {
        screen =
            screen.push(
                container(
                    row![
                    style::mark(Health::Attention),
                    text("No AI model is set up, so matches use the free keyword match and nothing is written for you.")
                        .size(style::BODY_SIZE)
                        .width(Fill),
                    button(text("Set one up").size(style::BODY_SIZE))
                        .style(style::primary_button)
                        .on_press(open("setup#ai")),
                ]
                    .spacing(ROW_GAP)
                    .align_y(Center),
                )
                .padding([12.0, CARD_PADDING])
                .width(Fill)
                .style(style::warning_bar),
            );
    }
    screen = screen.push(section("Status", rows(dashboard.rows.iter().map(status_row))));

    let findings = &overview.findings;
    let tiles = row(findings.counts.iter().map(stat_tile)).spacing(GAP);
    screen = screen.push(column![section_title("Your matches"), tiles].spacing(10));
    let best: Element<'_, Message> = if findings.best.is_empty() {
        container(
            text("No match on the shortlist yet.")
                .size(style::BODY_SIZE)
                .style(style::secondary),
        )
        .padding(CARD_PADDING)
        .into()
    } else {
        rows(findings.best.iter().map(best_row))
    };
    screen.push(section("Best on the shortlist", best)).into()
}

/// The page's title, with what's under it on the left and Match a job on the right.
fn heading<'a>(title: &'a str, under: Element<'a, Message>) -> Element<'a, Message> {
    row![
        column![text(title).font(style::semibold()).size(style::TITLE_SIZE), under]
            .spacing(4)
            .width(Fill),
        button(text("Match a job").size(style::BODY_SIZE))
            .padding([8.0, 16.0])
            .style(style::primary_button)
            .on_press(open("match")),
    ]
    .align_y(Center)
    .into()
}

/// The browser opening path.
fn open(path: &str) -> Message {
    Message::OpenPage(path.to_string())
}

/// One thing the app runs on: its mark, name and text, and its buttons on the right.
fn status_row(status: &StatusRow) -> Element<'_, Message> {
    let mut buttons = row![].spacing(8);
    if let Some(fix) = &status.fix {
        buttons = buttons.push(
            button(text(fix.label).size(style::BODY_SIZE))
                .style(style::standard_button)
                .on_press(open(fix.href)),
        );
    }
    if let Some(action) = &status.action {
        buttons = buttons.push(
            button(text(action.label).size(style::BODY_SIZE))
                .style(style::primary_button)
                .on_press(Message::Do(action.work)),
        );
    }
    row![
        style::mark(status.health),
        column![
            text(status.name).font(style::semibold()).size(style::BODY_SIZE),
            text(status.text.as_str())
                .size(style::CAPTION_SIZE)
                .style(style::secondary),
        ]
        .spacing(2)
        .width(Fill),
        buttons,
    ]
    .spacing(ROW_GAP)
    .align_y(Center)
    .padding([12.0, CARD_PADDING])
    .into()
}

/// A list and how many it holds, as a card that opens it.
fn stat_tile(count: &ListCount) -> Element<'_, Message> {
    button(
        column![
            text(count.count.to_string())
                .font(style::semibold())
                .size(style::STAT_SIZE),
            text(count.label).size(style::CAPTION_SIZE).style(style::secondary),
        ]
        .spacing(2),
    )
    .padding([12.0, 14.0])
    .width(Fill)
    .height(TILE_HEIGHT)
    .style(style::standard_button)
    .on_press(open(&count.href))
    .into()
}

/// A saved match on the shortlist, opening its page.
fn best_row(saved: &SavedMatchRow) -> Element<'_, Message> {
    button(
        row![
            column![
                text(saved.title.as_str())
                    .font(style::semibold())
                    .size(style::BODY_SIZE),
                text(saved.about.as_str())
                    .size(style::CAPTION_SIZE)
                    .style(style::secondary),
            ]
            .spacing(2)
            .width(Fill),
            text(saved.saved_on.as_str())
                .size(style::CAPTION_SIZE)
                .style(style::secondary),
            text("›").size(style::SECTION_SIZE).style(style::secondary),
        ]
        .spacing(ROW_GAP)
        .align_y(Center),
    )
    .padding([12.0, CARD_PADDING])
    .width(Fill)
    .style(style::row_button)
    .on_press(open(&format!("match/{}", saved.id)))
    .into()
}
