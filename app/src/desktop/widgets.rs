//! The pieces every screen is built from, so the screens look alike: a page title, a titled
//! card, rows in a card with hairlines between them, and a message bar.

use iced::widget::{Column, column, container, row, space, text};
use iced::{Center, Element, Fill};

use super::style;
use crate::dashboard::Health;

/// The padding inside a card.
pub const CARD_PADDING: f32 = 16.0;
/// A row's padding inside a card.
pub const ROW_PADDING: [f32; 2] = [12.0, CARD_PADDING];
/// Space between a heading and its card.
const HEADING_GAP: f32 = 10.0;

/// A page's title and the line under it.
pub fn page_title<'a, M: 'a>(title: &'a str, under: Element<'a, M>) -> Element<'a, M> {
    column![text(title).font(style::semibold()).size(style::TITLE_SIZE), under]
        .spacing(4)
        .width(Fill)
        .into()
}

/// Secondary text at body size, for the line under a title.
pub fn note<'a, M: 'a>(line: impl text::IntoFragment<'a>) -> Element<'a, M> {
    text(line).size(style::BODY_SIZE).style(style::secondary).into()
}

/// Secondary text at caption size, for the line under a field or a row's name.
pub fn caption<'a, M: 'a>(line: impl text::IntoFragment<'a>) -> Element<'a, M> {
    text(line).size(style::CAPTION_SIZE).style(style::secondary).into()
}

/// A section's heading.
pub fn section_title<'a, M: 'a>(title: &'a str) -> Element<'a, M> {
    text(title).font(style::semibold()).size(style::SECTION_SIZE).into()
}

/// A titled card.
pub fn section<'a, M: 'a>(title: &'a str, body: Element<'a, M>) -> Element<'a, M> {
    column![section_title(title), card(body)].spacing(HEADING_GAP).into()
}

/// A card around body, full width.
pub fn card<'a, M: 'a>(body: Element<'a, M>) -> Element<'a, M> {
    container(body).width(Fill).style(style::card).into()
}

/// Rows in a card, a hairline between each.
pub fn rows<'a, M: 'a>(items: impl Iterator<Item = Element<'a, M>>) -> Element<'a, M> {
    let mut list = Column::new();
    for (index, item) in items.enumerate() {
        if index > 0 {
            list = list.push(divider());
        }
        list = list.push(item);
    }
    list.into()
}

/// A one-pixel line across a card.
pub fn divider<'a, M: 'a>() -> Element<'a, M> {
    container(space()).width(Fill).height(1).style(style::divider).into()
}

/// A bar saying what just happened: its mark, and the text.
pub fn message_bar<'a, M: 'a>(health: Health, said: impl text::IntoFragment<'a>) -> Element<'a, M> {
    container(
        row![style::mark(health), text(said).size(style::BODY_SIZE).width(Fill)]
            .spacing(12)
            .align_y(Center),
    )
    .padding([10.0, 14.0])
    .width(Fill)
    .style(if health == Health::Attention {
        style::warning_bar
    } else {
        style::card
    })
    .into()
}

/// A labelled field, with its note under it.
pub fn field<'a, M: 'a>(label: &'a str, input: Element<'a, M>, note: Option<&'a str>) -> Element<'a, M> {
    let mut field = column![text(label).font(style::semibold()).size(style::CAPTION_SIZE), input].spacing(6);
    if let Some(note) = note {
        field = field.push(caption(note));
    }
    field.into()
}

/// What a form says is wrong with it, one line each.
pub fn errors<'a, M: 'a>(errors: &'a [String]) -> Element<'a, M> {
    Column::with_children(errors.iter().map(|error| {
        text(error.as_str())
            .size(style::CAPTION_SIZE)
            .style(style::health_text(Health::Attention))
            .into()
    }))
    .spacing(2)
    .into()
}
