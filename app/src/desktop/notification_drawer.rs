//! The notification drawer, which the bell in the status line opens from the bottom of the
//! window: what the app told the user (notifications), newest first, each opening the place it
//! is about. Opening it marks the ones it shows as read, so the bell's count goes.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Center, Element, Fill, Task};

use super::style;
use super::widgets::{self, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::matches::dates;
use crate::notifications::Kind;
use crate::notifications::notification_model::{self, Notification};

#[derive(Default)]
pub struct State {
    /// None until read; Err says why the list couldn't be read.
    rows: Option<Result<Vec<NotificationRow>, String>>,
}

/// A notification as its row shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationRow {
    pub title: String,
    pub text: String,
    /// "3 Nov 10:42".
    pub told_at: String,
    pub link: String,
    /// The mark: attention for a failed scan, ready for good news, neutral for the rest.
    pub health: Health,
    /// Not read before this visit, so shown in bold.
    pub new: bool,
}

impl NotificationRow {
    fn of(notification: Notification) -> Self {
        NotificationRow {
            health: health_of(notification.kind),
            told_at: dates::short_moment(notification.created_at),
            new: !notification.read,
            title: notification.title,
            text: notification.text,
            link: notification.link,
        }
    }
}

fn health_of(kind: Option<Kind>) -> Health {
    match kind {
        Some(Kind::ScanFailed) => Health::Attention,
        Some(Kind::GoodMatch | Kind::ScanFinished) => Health::Ready,
        Some(Kind::JobsClosed) | None => Health::Neutral,
    }
}

#[derive(Clone)]
pub enum Msg {
    Load,
    Loaded(Result<Vec<NotificationRow>, String>),
    /// A row was clicked: the window opens its link, and the drawer closes.
    Open(String),
    /// Close was clicked.
    Close,
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            return runner.run(
                |context| async move {
                    let db = context.db();
                    let listed = notification_model::newest(db).await?;
                    if let Some(newest) = listed.first() {
                        notification_model::mark_read(db, newest.id, chrono::Utc::now().timestamp()).await?;
                    }
                    Ok(listed.into_iter().map(NotificationRow::of).collect())
                },
                |read| {
                    Msg::Loaded(read.and_then(|listed: sqlx::Result<_>| {
                        listed.map_err(|e| {
                            crate::log_line!("notifications: the list couldn't be read: {e}");
                            format!("The notifications couldn't be read: {e}")
                        })
                    }))
                },
            );
        }
        Msg::Loaded(rows) => state.rows = Some(rows),
        // The shell opens it, and closes the drawer.
        Msg::Open(_) | Msg::Close => {}
    }
    Task::none()
}

/// The drawer's height: about five notifications, with the page still showing above it.
const DRAWER_HEIGHT: f32 = 360.0;
/// Space between the drawer and the window's sides.
const DRAWER_MARGIN: f32 = 16.0;

/// The drawer: its header with Close, and the notifications under it, scrolled when there are
/// more than fit.
pub fn view(state: &State) -> Element<'_, Msg> {
    let header = row![
        column![
            widgets::section_title("Notifications"),
            widgets::caption(format!(
                "Good matches the job scan found, and how each scan went. Kept for {} days.",
                notification_model::KEEP_DAYS
            )),
        ]
        .spacing(2)
        .width(Fill),
        button(text("Close").size(style::BODY_SIZE))
            .padding([4.0, 10.0])
            .style(style::subtle_button(false))
            .on_press(Msg::Close),
    ]
    .align_y(Center)
    .padding(ROW_PADDING);
    let body: Element<'_, Msg> = match &state.rows {
        None => container(widgets::note("Reading…")).padding(ROW_PADDING).into(),
        Some(Err(why)) => container(widgets::errors_line(why.as_str()))
            .padding(ROW_PADDING)
            .into(),
        Some(Ok(rows)) if rows.is_empty() => {
            container(widgets::note("Nothing yet. The job scan tells you here what it finds."))
                .padding(ROW_PADDING)
                .into()
        }
        Some(Ok(rows)) => scrollable(widgets::rows(rows.iter().map(notification_row)))
            .height(Fill)
            .into(),
    };
    let drawer = column![header, widgets::divider(), body];
    container(container(drawer).width(Fill).height(DRAWER_HEIGHT).style(style::card))
        .padding(iced::Padding::from([0.0, DRAWER_MARGIN]).top(DRAWER_MARGIN))
        .width(Fill)
        .into()
}

fn notification_row(row: &NotificationRow) -> Element<'_, Msg> {
    let title =
        text(row.title.as_str())
            .size(style::BODY_SIZE)
            .font(if row.new { style::semibold() } else { style::font() });
    let mut lines = column![title].spacing(2).width(Fill);
    if !row.text.is_empty() {
        lines = lines.push(widgets::caption(row.text.as_str()));
    }
    let line = row![style::mark(row.health), lines, widgets::caption(row.told_at.as_str())]
        .spacing(GAP)
        .align_y(Center);
    button(line)
        .padding(ROW_PADDING)
        .width(Fill)
        .style(style::row_button)
        .on_press(Msg::Open(row.link.clone()))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_shows_a_failed_scan_as_needing_attention_and_unread_ones_as_new() {
        let row = NotificationRow::of(Notification {
            id: 1,
            kind: Some(Kind::ScanFailed),
            title: "The job scan failed".to_string(),
            text: "No key.".to_string(),
            link: "job_scan".to_string(),
            created_at: 0,
            read: false,
        });
        assert_eq!(row.health, Health::Attention);
        assert!(row.new);
        assert_eq!(health_of(Some(Kind::GoodMatch)), Health::Ready);
        assert_eq!(health_of(None), Health::Neutral);
    }
}
