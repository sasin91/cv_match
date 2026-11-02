//! The Learned terms screen: the labels the taxonomy learned from matches (taxonomy_store), each
//! with the term it names, how many matches taught it and when it was first seen, with Approve,
//! Reject and Edit. A search finds any label, a shipped one too, to edit or reject it.

use iced::widget::{Column, button, column, container, row, text, text_input};
use iced::{Center, Element, Fill, Length, Task};

use super::style;
use super::widgets::{self, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::taxonomy_store::{self, ACTIVE, CANDIDATE, ListedLabel, REJECTED, Review, SHIPPED};

/// The search field's width.
const SEARCH_WIDTH: f32 = 320.0;
/// The Term column's width.
const TERM_WIDTH: f32 = 200.0;
/// The Status column's width.
const STATUS_WIDTH: f32 = 90.0;
/// The Seen column's width.
const SEEN_WIDTH: f32 = 110.0;

#[derive(Default)]
pub struct State {
    /// None until read; Err says why the list couldn't be read.
    labels: Option<Result<Vec<ListedLabel>, String>>,
    /// The search's words; empty lists the learned labels.
    search: String,
    /// The label being edited, and its text so far.
    editing: Option<(i64, String)>,
    /// What the last review came to.
    said: Option<(Health, String)>,
}

#[derive(Clone)]
pub enum Msg {
    Load,
    Loaded(Result<Vec<ListedLabel>, String>),
    Search(String),
    Edit(i64, String),
    EditText(String),
    CancelEdit,
    Review(i64, Review),
    Reviewed(Result<String, String>),
}

pub fn update(state: &mut State, msg: Msg, runner: &Runner) -> Task<Msg> {
    match msg {
        Msg::Load => {
            let search = state.search.trim().to_string();
            return runner.run(
                move |context| async move {
                    let listed = if search.is_empty() {
                        taxonomy_store::learned_labels(context.db()).await
                    } else {
                        taxonomy_store::find_labels(context.db(), &search).await
                    };
                    listed.map_err(|e| {
                        crate::log_line!("learned_terms: the labels couldn't be read: {e}");
                        format!("The labels couldn't be read: {e}")
                    })
                },
                |read| Msg::Loaded(read.and_then(|listed| listed)),
            );
        }
        Msg::Loaded(labels) => state.labels = Some(labels),
        Msg::Search(words) => {
            state.search = words;
            return Task::done(Msg::Load);
        }
        Msg::Edit(id, text) => state.editing = Some((id, text)),
        Msg::EditText(text) => {
            if let Some((_, editing)) = &mut state.editing {
                *editing = text;
            }
        }
        Msg::CancelEdit => state.editing = None,
        Msg::Review(id, review) => {
            state.editing = None;
            return runner.run(
                move |context| async move { taxonomy_store::review(&context, id, review).await },
                |answer| Msg::Reviewed(answer.and_then(|said| said)),
            );
        }
        Msg::Reviewed(said) => {
            state.said = Some(match said {
                Ok(said) => (Health::Ready, said),
                Err(why) => (Health::Attention, why),
            });
            return Task::done(Msg::Load);
        }
    }
    Task::none()
}

pub fn view(state: &State) -> Element<'_, Msg> {
    let title = widgets::page_title(
        "Learned terms",
        widgets::note(
            "Phrases the taxonomy learned from your matches. One is used once two matches taught it, or when you approve it.",
        ),
    );
    let search = text_input("Find any label, shipped ones too", &state.search)
        .on_input(Msg::Search)
        .size(style::BODY_SIZE)
        .padding(8)
        .style(style::text_field)
        .width(SEARCH_WIDTH);
    let mut screen = Column::new().spacing(GAP * 1.5).push(title).push(search);
    if let Some((health, said)) = &state.said {
        screen = screen.push(widgets::message_bar(*health, said.as_str()));
    }
    let labels = match &state.labels {
        None => return screen.push(widgets::note("Reading the labels…")).into(),
        Some(Err(why)) => {
            return screen
                .push(widgets::message_bar(Health::Attention, why.as_str()))
                .into();
        }
        Some(Ok(labels)) => labels,
    };
    if labels.is_empty() {
        let empty = if state.search.trim().is_empty() {
            "Nothing learned yet. An AI match teaches the taxonomy what it couldn't find by itself."
        } else {
            "No label has those words."
        };
        return screen
            .push(widgets::card(
                container(widgets::note(empty)).padding(ROW_PADDING).into(),
            ))
            .into();
    }
    let list = column![
        header(),
        widgets::divider(),
        widgets::rows(labels.iter().map(|label| label_row(label, state.editing.as_ref())))
    ];
    screen.push(widgets::card(list.into())).into()
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
            name("Label", Fill),
            name("Term", Length::Fixed(TERM_WIDTH)),
            name("Status", Length::Fixed(STATUS_WIDTH)),
            name("Seen", Length::Fixed(SEEN_WIDTH)),
            name("", Length::Shrink),
        ]
        .spacing(GAP),
    )
    .padding(ROW_PADDING)
    .into()
}

/// A label: its text (or its edit field), its term, its status, how often and since when it was
/// seen, and what can be done to it.
fn label_row<'a>(label: &'a ListedLabel, editing: Option<&'a (i64, String)>) -> Element<'a, Msg> {
    let cell = |line: String, width: Length| text(line).size(style::BODY_SIZE).width(width);
    let edited = editing.filter(|(id, _)| *id == label.id);
    let name: Element<'_, Msg> = match edited {
        Some((_, text)) => text_input("Label", text)
            .on_input(Msg::EditText)
            .on_submit(Msg::Review(label.id, Review::Rename(text.clone())))
            .size(style::BODY_SIZE)
            .padding(6)
            .style(style::text_field)
            .width(Fill)
            .into(),
        None => cell(format!("{} ({})", label.text, label.lang), Fill).into(),
    };
    let line = row![
        name,
        cell(label.term_label.clone(), Length::Fixed(TERM_WIDTH)),
        cell(status_text(label).to_string(), Length::Fixed(STATUS_WIDTH)),
        cell(seen_text(label), Length::Fixed(SEEN_WIDTH)),
        actions(label, edited),
    ]
    .spacing(GAP)
    .align_y(Center);
    container(line).padding(ROW_PADDING).width(Fill).into()
}

/// "Candidate", "Active", "Rejected", or "Shipped" for one of the index's own.
fn status_text(label: &ListedLabel) -> &'static str {
    match (label.source.as_str(), label.status.as_str()) {
        (SHIPPED, _) => "Shipped",
        (_, ACTIVE) => "Active",
        (_, CANDIDATE) => "Candidate",
        (_, REJECTED) => "Rejected",
        _ => "",
    }
}

/// "2 × since 1 Nov", or "" for a label no match taught.
fn seen_text(label: &ListedLabel) -> String {
    match label.first_seen_at {
        Some(at) => format!("{} × since {}", label.sightings, crate::matches::dates::day(at)),
        None => String::new(),
    }
}

/// The buttons for a label: Save and Cancel while it is edited; otherwise Approve (unless
/// active), Reject (unless rejected) and Edit.
fn actions<'a>(label: &'a ListedLabel, edited: Option<&'a (i64, String)>) -> Element<'a, Msg> {
    let action = |name: &'a str, msg: Msg| {
        button(text(name).size(style::BODY_SIZE))
            .style(style::standard_button)
            .on_press(msg)
    };
    if let Some((_, text)) = edited {
        return row![
            action("Save", Msg::Review(label.id, Review::Rename(text.clone()))),
            action("Cancel", Msg::CancelEdit),
        ]
        .spacing(GAP / 2.0)
        .into();
    }
    let mut buttons = row![].spacing(GAP / 2.0);
    if label.status != ACTIVE {
        buttons = buttons.push(action("Approve", Msg::Review(label.id, Review::Approve)));
    }
    if label.status != REJECTED {
        buttons = buttons.push(action("Reject", Msg::Review(label.id, Review::Reject)));
    }
    buttons
        .push(action("Edit", Msg::Edit(label.id, label.text.clone())))
        .into()
}
