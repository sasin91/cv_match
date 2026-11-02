//! The Learned terms screen: the labels the taxonomy learned from matches (taxonomy_store), each
//! with the term it names, how many matches taught it and when it was first seen, with Approve,
//! Reject and Edit. Why shows each match that taught it: the post it was found in, and the model's
//! reason (the judge's quote of the CV and its verdict). A search finds any label, a shipped one
//! too, to edit or reject it.

use iced::widget::{Column, button, column, container, row, text, text_input};
use iced::{Center, Element, Fill, Length, Task};

use super::style;
use super::widgets::{self, ROW_PADDING};
use super::{GAP, Runner};
use crate::dashboard::Health;
use crate::taxonomy_store::{
    self, ACTIVE, CANDIDATE, FROM_JUDGE, LabelSighting, ListedLabel, REJECTED, Review, SHIPPED,
};

/// The search field's width.
const SEARCH_WIDTH: f32 = 320.0;
/// The Status column's width.
const STATUS_WIDTH: f32 = 80.0;
/// The Seen column's width: "12 ×, 30 Nov 2026" on one line.
const SEEN_WIDTH: f32 = 120.0;
/// The buttons' column width: Why, Approve, Reject and Edit side by side. The same in the header and
/// every row, whichever buttons a row has, so the columns line up.
const ACTIONS_WIDTH: f32 = 260.0;

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
    /// The label whose Why is open, and its sightings once read (Err says why they couldn't be).
    why: Option<OpenWhy>,
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
    /// Opens or closes a label's Why.
    Why(i64),
    WhyRead(i64, Result<Vec<LabelSighting>, String>),
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
        Msg::Why(id) => {
            if state.why.as_ref().is_some_and(|(open, _)| *open == id) {
                state.why = None;
                return Task::none();
            }
            state.why = Some((id, None));
            return runner.run(
                move |context| async move {
                    taxonomy_store::sightings(context.db(), id).await.map_err(|e| {
                        crate::log_line!("learned_terms: the matches that taught label {id} couldn't be read: {e}");
                        format!("The matches that taught it couldn't be read: {e}")
                    })
                },
                move |read| Msg::WhyRead(id, read.and_then(|sightings| sightings)),
            );
        }
        Msg::WhyRead(id, sightings) => {
            if let Some((open, read)) = &mut state.why
                && *open == id
            {
                *read = Some(sightings);
            }
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
        widgets::rows(
            labels
                .iter()
                .map(|label| label_row(label, state.editing.as_ref(), state.why.as_ref()))
        )
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
            name("Label, and the term it names", Fill),
            name("Status", Length::Fixed(STATUS_WIDTH)),
            name("Seen", Length::Fixed(SEEN_WIDTH)),
            name("", Length::Fixed(ACTIONS_WIDTH)),
        ]
        .spacing(GAP),
    )
    .padding(ROW_PADDING)
    .into()
}

/// The label whose Why is open, and its sightings once read.
type OpenWhy = (i64, Option<Result<Vec<LabelSighting>, String>>);

/// A label: its text (or its edit field), its term, its status, how often and since when it was
/// seen, and what can be done to it; under it, its Why when open.
fn label_row<'a>(
    label: &'a ListedLabel,
    editing: Option<&'a (i64, String)>,
    why: Option<&'a OpenWhy>,
) -> Element<'a, Msg> {
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
        None => column![
            text(format!("{} ({})", label.text, label.lang)).size(style::BODY_SIZE),
            widgets::caption(format!("names {}", label.term_label)),
        ]
        .spacing(2)
        .width(Fill)
        .into(),
    };
    let line = row![
        name,
        cell(status_text(label).to_string(), Length::Fixed(STATUS_WIDTH)),
        cell(seen_text(label), Length::Fixed(SEEN_WIDTH)),
        actions_cell(actions(label, edited)),
    ]
    .spacing(GAP)
    .align_y(Center);
    let mut shown = Column::new().spacing(GAP / 2.0).push(line);
    if let Some((_, read)) = why.filter(|(id, _)| *id == label.id) {
        shown = shown.push(why_panel(read.as_ref()));
    }
    container(shown).padding(ROW_PADDING).width(Fill).into()
}

/// The matches that taught a label: for each, where it was found and why.
fn why_panel<'a>(read: Option<&'a Result<Vec<LabelSighting>, String>>) -> Element<'a, Msg> {
    let sightings = match read {
        None => return widgets::note("Reading the matches that taught it…"),
        Some(Err(why)) => return widgets::message_bar(Health::Attention, why.as_str()),
        Some(Ok(sightings)) if sightings.is_empty() => {
            return widgets::note("No match taught it: it was approved or written by hand.");
        }
        Some(Ok(sightings)) => sightings,
    };
    let mut panel = Column::new().spacing(GAP);
    for sighting in sightings {
        let mut lines = Column::new()
            .spacing(2)
            .push(text(found_in(sighting)).size(style::BODY_SIZE));
        if let Some(url) = sighting.job_url.as_deref().filter(|url| !url.is_empty()) {
            lines = lines.push(widgets::caption(url));
        }
        lines = lines.push(widgets::note(why_learned(sighting)));
        panel = panel.push(lines);
    }
    container(panel).padding([0.0, GAP]).into()
}

/// "Found in match 349, 2 Nov 2026: Social- og sundhedsassistent at Region Sjælland".
fn found_in(sighting: &LabelSighting) -> String {
    let at = if sighting.company.is_empty() {
        String::new()
    } else {
        format!(" at {}", sighting.company)
    };
    format!(
        "Found in match {}, {}: {}{at}",
        sighting.match_id,
        crate::matches::dates::day(sighting.seen_at),
        sighting.job_title
    )
}

/// Why the match taught it: the judge's verdict, quote of the CV and reason, or the model's read
/// of the post naming the term in English; who answered.
fn why_learned(sighting: &LabelSighting) -> String {
    let by = if sighting.model.is_empty() {
        String::new()
    } else {
        format!(" ({})", sighting.model)
    };
    if sighting.learned_from != FROM_JUDGE {
        return format!(
            "The model{by} read this requirement in the post, and its English for it is one of the term's names."
        );
    }
    let verdict = sighting.verdict.as_deref().unwrap_or("met");
    let quote = match sighting.evidence.as_deref().filter(|quote| !quote.is_empty()) {
        Some(quote) => format!(", quoting your CV: \"{quote}\""),
        None => String::new(),
    };
    let reason = match sighting.reason.as_deref().filter(|reason| !reason.is_empty()) {
        Some(reason) => format!(" Its reason: {reason}"),
        None => String::new(),
    };
    format!(
        "The judge{by} said the requirement is {verdict}{quote}, and the quote holds one of your CV's items that names the term.{reason}"
    )
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

/// "2 ×, 1 Nov 2026": how many matches taught it, and the first one's day; "" for a label no
/// match taught.
fn seen_text(label: &ListedLabel) -> String {
    match label.first_seen_at {
        Some(at) => format!("{} ×, {}", label.sightings, crate::matches::dates::day(at)),
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
    if label.sightings > 0 {
        buttons = buttons.push(action("Why", Msg::Why(label.id)));
    }
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

/// A row's buttons in the fixed-width column, at its right edge.
fn actions_cell<'a>(buttons: Element<'a, Msg>) -> Element<'a, Msg> {
    container(buttons).width(ACTIONS_WIDTH).align_x(iced::Right).into()
}
