//! The Task manager screen: what JobFinder runs now (task_manager::rows), how long each has run,
//! and a right-click menu on each row to stop it. The list is read again every READ_EVERY while
//! the screen shows.

use iced::widget::{Column, button, column, container, mouse_area, pin, row, space, stack, text};
use iced::{Center, Element, Fill, Length, Point, Task};

use super::style;
use super::widgets::{self, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::task_manager::{self, Task as Work, TaskRow};

/// How often the list is read again while the screen shows (every second, so Running for ticks).
const READ_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
/// How far a row is set in for each row it is under.
const INDENT: f32 = 20.0;
/// The Kind column's width.
const KIND_WIDTH: f32 = 110.0;
/// The Process column's width.
const PROCESS_WIDTH: f32 = 80.0;
/// The Started column's width.
const STARTED_WIDTH: f32 = 70.0;
/// The Running for column's width.
const RUNNING_FOR_WIDTH: f32 = 100.0;
/// The right-click menu's width.
const MENU_WIDTH: f32 = 160.0;
/// Seconds in a minute, which Running for is said in.
const MINUTE: u64 = 60;
/// Seconds in an hour, which Running for is said in from the first hour on.
const HOUR: u64 = 3600;

#[derive(Default)]
pub struct State {
    /// None until read.
    rows: Option<Vec<TaskRow>>,
    /// When the rows were read, in unix seconds.
    read_at: i64,
    /// The row whose menu is open, and where (in the list's own coordinates).
    menu: Option<(Work, Point)>,
    /// Where the pointer is over the list, for the menu to open at.
    pointer: Point,
    /// What the last stop came to.
    said: Option<(Health, String)>,
    /// The screen shows: the list is read again every READ_EVERY.
    shown: bool,
    /// A read is on its way, so two aren't.
    reading: bool,
    /// The next read is waited for, so a second visit doesn't start a second round.
    waiting: bool,
}

#[derive(Clone)]
pub enum Msg {
    Load,
    Loaded(Vec<TaskRow>),
    /// READ_EVERY went by.
    Poll,
    /// The pointer moved over the list.
    Pointer(Point),
    OpenMenu(Work),
    CloseMenu,
    Stop(Work),
    Said(Result<String, String>),
}

impl State {
    /// The screen is shown: read the list, and keep reading it.
    pub fn show(&mut self) {
        self.shown = true;
        self.menu = None;
        self.said = None;
    }

    /// Another screen is shown: stop reading the list.
    pub fn hide(&mut self) {
        self.shown = false;
        self.menu = None;
    }
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            if state.reading || !state.shown {
                return Task::none();
            }
            state.reading = true;
            return runner.run(
                |context| async move { task_manager::rows(&context).await },
                |read| Msg::Loaded(read.unwrap_or_default()),
            );
        }
        Msg::Loaded(rows) => {
            state.reading = false;
            state.read_at = chrono::Utc::now().timestamp();
            // A row that finished takes its menu with it.
            if let Some((task, _)) = state.menu
                && !rows.iter().any(|row| row.task == task)
            {
                state.menu = None;
            }
            state.rows = Some(rows);
            if state.shown && !state.waiting {
                state.waiting = true;
                return runner.run(|_| tokio::time::sleep(READ_EVERY), |_| Msg::Poll);
            }
        }
        Msg::Poll => {
            state.waiting = false;
            return Task::done(Msg::Load);
        }
        Msg::Pointer(at) => state.pointer = at,
        Msg::OpenMenu(task) => state.menu = Some((task, state.pointer)),
        Msg::CloseMenu => state.menu = None,
        Msg::Stop(task) => {
            state.menu = None;
            return runner.run(
                move |context| async move { task_manager::stop(&context, task).await },
                |answer| Msg::Said(answer.and_then(|said| said.map_err(|e| e.message))),
            );
        }
        Msg::Said(said) => {
            state.said = Some(match said {
                Ok(said) => (Health::Ready, said),
                Err(why) => {
                    crate::log_line!("task_manager: stopping failed: {why}");
                    (Health::Attention, why)
                }
            });
        }
    }
    Task::none()
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Task manager",
        widgets::note("What JobFinder runs now, and for how long. Right-click a row to stop it."),
    );
    let mut screen = Column::new().spacing(GAP * 1.5).push(title);
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    let Some(rows) = &state.rows else {
        return screen.push(widgets::note("Looking at what runs…")).into();
    };
    if rows.is_empty() {
        return screen
            .push(widgets::card(
                container(widgets::note("Nothing runs now."))
                    .padding(ROW_PADDING)
                    .into(),
            ))
            .into();
    }
    let list = column![
        header(),
        widgets::divider(),
        widgets::rows(rows.iter().map(|row| task_row(row, state.read_at)))
    ];
    let list = widgets::card(list.into());
    // The menu goes over the list only while it is open.
    let list = match open_menu(state, rows) {
        Some((row, at)) => stack![list, menu_layer(row, at)].into(),
        None => list,
    };
    let list = mouse_area(list).on_move(Msg::Pointer);
    screen.push(list).into()
}

/// The row whose menu is open, and where, while that row is still listed.
fn open_menu<'a>(state: &State, rows: &'a [TaskRow]) -> Option<(&'a TaskRow, Point)> {
    let (task, at) = state.menu?;
    let row = rows.iter().find(|row| row.task == task)?;
    Some((row, at))
}

/// The columns' names.
fn header<'a>() -> Element<'a, Msg> {
    let name = |label: &'a str, width: Length| {
        text(label)
            .size(style::CAPTION_SIZE)
            .style(style::secondary)
            .width(width)
    };
    container(
        row![
            name("Kind", Length::Fixed(KIND_WIDTH)),
            name("What", Fill),
            name("Process", Length::Fixed(PROCESS_WIDTH)),
            name("Started", Length::Fixed(STARTED_WIDTH)),
            name("Running for", Length::Fixed(RUNNING_FOR_WIDTH)),
        ]
        .spacing(GAP),
    )
    .padding(ROW_PADDING)
    .into()
}

/// A row: its kind, what it works on (set in under the row it is part of), its process, when it
/// started and for how long it has run at now. Right-click opens its menu.
fn task_row(row: &TaskRow, now: i64) -> Element<'_, Msg> {
    let cell = |line: String, width: Length| text(line).size(style::BODY_SIZE).width(width);
    let what = if row.stopping {
        format!("{} (stopping)", row.what)
    } else {
        row.what.clone()
    };
    let what = row![
        space().width(INDENT * row.depth as f32),
        text(what).size(style::BODY_SIZE).width(Fill)
    ];
    let line = row![
        cell(row.kind.to_string(), Length::Fixed(KIND_WIDTH)),
        container(what).width(Fill),
        cell(
            row.process_id.map(|id| id.to_string()).unwrap_or_default(),
            Length::Fixed(PROCESS_WIDTH)
        ),
        cell(
            row.started_at.map(crate::matches::dates::clock).unwrap_or_default(),
            Length::Fixed(STARTED_WIDTH)
        ),
        cell(
            row.running_for(now).map(running_for_text).unwrap_or_default(),
            Length::Fixed(RUNNING_FOR_WIDTH)
        ),
    ]
    .spacing(GAP)
    .align_y(Center);
    mouse_area(container(line).padding(ROW_PADDING).width(Fill))
        .on_right_press(Msg::OpenMenu(row.task))
        .on_press(Msg::CloseMenu)
        .into()
}

/// The menu open on row, at at; a click anywhere else closes it.
fn menu_layer(row: &TaskRow, at: Point) -> Element<'_, Msg> {
    let item: Element<'_, Msg> = match (row.stop, row.stopping) {
        (Some(_), true) => menu_item("Stopping…", None),
        (Some(stop), false) => menu_item(stop, Some(Msg::Stop(row.task))),
        (None, _) => menu_item("Can't be stopped", None),
    };
    let menu = container(column![item].width(MENU_WIDTH)).padding(4).style(style::card);
    mouse_area(pin(menu).x(at.x).y(at.y))
        .on_press(Msg::CloseMenu)
        .on_right_press(Msg::CloseMenu)
        .into()
}

fn menu_item(label: &str, on_press: Option<Msg>) -> Element<'_, Msg> {
    let label = text(label).size(style::BODY_SIZE);
    let label = if on_press.is_some() {
        label
    } else {
        label.style(style::secondary)
    };
    button(label)
        .padding([6.0, 12.0])
        .width(Fill)
        .style(style::row_button)
        .on_press_maybe(on_press)
        .into()
}

/// "1 h 04 min", "3 min 12 s", "12 s".
fn running_for_text(running_for: std::time::Duration) -> String {
    let seconds = running_for.as_secs();
    if seconds >= HOUR {
        format!("{} h {:02} min", seconds / HOUR, seconds % HOUR / MINUTE)
    } else if seconds >= MINUTE {
        format!("{} min {:02} s", seconds / MINUTE, seconds % MINUTE)
    } else {
        format!("{seconds} s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn running_for_is_said_in_the_largest_units_that_fit() {
        assert_eq!(running_for_text(Duration::from_secs(12)), "12 s");
        assert_eq!(running_for_text(Duration::from_secs(192)), "3 min 12 s");
        assert_eq!(running_for_text(Duration::from_secs(3840)), "1 h 04 min");
    }
}
