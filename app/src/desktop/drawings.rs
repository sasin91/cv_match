//! The web page's drawings in the window: flat pine and marigold scenes on a wash, in light or dark as
//! the system is set. The SVG files in assets/drawings use the web page's colour names
//! (`var(--sage)`), which are filled in here for each look, so the page and the window share one
//! file per drawing.

use std::sync::LazyLock;

use iced::widget::canvas::{Canvas, Frame, Geometry, Program};
use iced::widget::svg::Handle;
use iced::{Element, Length, Rectangle, Renderer, Size, Theme, mouse};

/// The web page's colour names and their light and dark values (its `:root` tokens).
const TOKENS: [(&str, u32, u32); 8] = [
    ("--bg", 0xEEF1EF, 0x0F1B24),
    ("--paper", 0xF9FBFA, 0x16252F),
    ("--ink", 0x142430, 0xE6EEF0),
    ("--soft", 0x526270, 0x93A6B1),
    ("--line", 0xD5DDDB, 0x263844),
    ("--sage", 0x2E6B5B, 0x6FB59C),
    ("--clay", 0xE2A33B, 0xF0B952),
    ("--wash", 0xDEE7E3, 0x1B2C37),
];

/// One drawing, ready in both looks, and its size in the window.
pub struct Drawing {
    light: Handle,
    dark: Handle,
    size: Size,
}

impl Drawing {
    fn new(source: &str, size: Size) -> Self {
        Self {
            light: Handle::from_memory(painted(source, false).into_bytes()),
            dark: Handle::from_memory(painted(source, true).into_bytes()),
            size,
        }
    }
}

/// The SVG with each `var(--name)` replaced by its light or dark colour.
fn painted(source: &str, dark: bool) -> String {
    TOKENS
        .iter()
        .fold(source.to_string(), |svg, (name, light_rgb, dark_rgb)| {
            let rgb = if dark { dark_rgb } else { light_rgb };
            svg.replace(&format!("var({name})"), &format!("#{rgb:06X}"))
        })
}

// Each drawing is made once, on first use, and never changes after (read-only static).

/// A laptop of job cards beside a mug and a plant: the web page's top drawing.
pub static HERO: LazyLock<Drawing> = LazyLock::new(|| {
    Drawing::new(
        include_str!("../../../assets/drawings/hero.svg"),
        Size::new(200.0, 150.0),
    )
});
/// Job cards under a magnifying glass: looking for the jobs you'd have missed.
pub static FINDS: LazyLock<Drawing> = LazyLock::new(|| {
    Drawing::new(
        include_str!("../../../assets/drawings/finds.svg"),
        Size::new(240.0, 160.0),
    )
});
/// A mug and a plant: nothing to do, kick back.
pub static REST: LazyLock<Drawing> = LazyLock::new(|| {
    Drawing::new(
        include_str!("../../../assets/drawings/rest.svg"),
        Size::new(240.0, 160.0),
    )
});
/// The computer and a phone on a private, locked line, a QR card for pairing.
pub static PHONE: LazyLock<Drawing> = LazyLock::new(|| {
    Drawing::new(
        include_str!("../../../assets/drawings/phone.svg"),
        Size::new(280.0, 200.0),
    )
});

/// Draws the drawing in the theme's look; a canvas, since only drawing knows the theme.
struct Picture(&'static Drawing);

impl<Message> Program<Message> for Picture {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let handle = if theme.extended_palette().is_dark {
            &self.0.dark
        } else {
            &self.0.light
        };
        frame.draw_svg(Rectangle::with_size(bounds.size()), handle);
        vec![frame.into_geometry()]
    }
}

/// The drawing at its size.
pub fn drawing<'a, Message: 'a>(drawing: &'static Drawing) -> Element<'a, Message> {
    Canvas::new(Picture(drawing))
        .width(Length::Fixed(drawing.size.width))
        .height(Length::Fixed(drawing.size.height))
        .into()
}

/// An empty list: the drawing, and the line saying why it's empty, centred (for inside a card).
pub fn empty<'a, Message: 'a>(picture: &'static Drawing, line: &'a str) -> Element<'a, Message> {
    let body = iced::widget::column![drawing(picture), super::widgets::note(line)]
        .spacing(EMPTY_GAP)
        .align_x(iced::Center);
    iced::widget::container(body)
        .padding(EMPTY_PADDING)
        .center_x(Length::Fill)
        .into()
}

/// Space between an empty list's drawing and its line.
const EMPTY_GAP: f32 = 8.0;
/// Space around an empty list's drawing, inside the card.
const EMPTY_PADDING: f32 = 24.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_colour_name_in_the_drawings_is_filled_in() {
        for source in [
            include_str!("../../../assets/drawings/hero.svg"),
            include_str!("../../../assets/drawings/finds.svg"),
            include_str!("../../../assets/drawings/rest.svg"),
            include_str!("../../../assets/drawings/phone.svg"),
        ] {
            for dark in [false, true] {
                assert!(
                    !painted(source, dark).contains("var("),
                    "a colour name is left in a drawing"
                );
            }
        }
    }
}
