//! The window's look, after Windows 11's (Fluent): a light grey page, white cards with a hairline
//! border and a soft shadow, Windows' blue as the accent, and status marks on soft tinted pills.
//! Every colour and size the screens use is named here, so they stay one design.

use iced::border::Radius;
use iced::overlay::menu;
use iced::widget::{button, checkbox, container, pick_list, text, text_editor, text_input};
use iced::{Background, Border, Color, Element, Font, Shadow, Theme, Vector, font};

use crate::dashboard::Health;

/// The colour from its hex RGB, as design tools give it.
const fn hex(rgb: u32) -> Color {
    Color::from_rgb(
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    )
}

/// The page behind the cards (Windows 11's Mica, light).
pub const PAGE: Color = hex(0xF3F3F3);
/// A card's face.
pub const CARD: Color = hex(0xFFFFFF);
/// A card's hairline border.
pub const CARD_BORDER: Color = hex(0xE5E5E5);
/// The line between rows inside a card.
pub const DIVIDER: Color = hex(0xEDEDED);
/// Body text.
pub const TEXT: Color = hex(0x1A1A1A);
/// Secondary text: notes, captions, paths.
pub const TEXT_SECONDARY: Color = hex(0x5F5F5F);
/// Windows' accent blue.
pub const ACCENT: Color = hex(0x005FB8);
/// The accent under the pointer.
const ACCENT_HOVER: Color = hex(0x196EBF);
/// The accent pressed.
const ACCENT_PRESSED: Color = hex(0x3C82C9);
/// A plain button's face under the pointer, and the selected sidebar item.
const SUBTLE_HOVER: Color = hex(0xEAEAEA);
/// A plain button pressed.
const SUBTLE_PRESSED: Color = hex(0xE0E0E0);

/// Ready: green text on a green tint.
const SUCCESS: Color = hex(0x0F7B0F);
const SUCCESS_TINT: Color = hex(0xDFF6DD);
/// Needs attention: amber text on an amber tint, as Windows' warning bar.
const WARNING: Color = hex(0x9D5D00);
const WARNING_TINT: Color = hex(0xFFF4CE);
const WARNING_BORDER: Color = hex(0xF2D27A);
/// Something failed.
const DANGER: Color = hex(0xC42B1C);
/// A note: grey on a grey tint.
const NEUTRAL: Color = hex(0x616161);
const NEUTRAL_TINT: Color = hex(0xF0F0F0);

/// Corner radius of cards and the warning bar.
pub const CARD_RADIUS: f32 = 8.0;
/// Corner radius of buttons and sidebar items.
const CONTROL_RADIUS: f32 = 4.0;

/// The page title's size.
pub const TITLE_SIZE: f32 = 28.0;
/// A card's heading.
pub const SECTION_SIZE: f32 = 16.0;
/// Body text.
pub const BODY_SIZE: f32 = 14.0;
/// Captions and notes.
pub const CAPTION_SIZE: f32 = 12.0;
/// A big number on a stat tile.
pub const STAT_SIZE: f32 = 24.0;

/// The window's theme: its palette, which iced's own widgets (scrollbars, focus) take colours from.
pub fn theme() -> Theme {
    Theme::custom(
        "Recruiter",
        iced::theme::Palette {
            background: PAGE,
            text: TEXT,
            primary: ACCENT,
            success: SUCCESS,
            warning: WARNING,
            danger: DANGER,
        },
    )
}

/// Windows 11's own font; elsewhere the system's sans-serif.
pub fn font() -> Font {
    if cfg!(windows) {
        Font::with_name("Segoe UI")
    } else {
        Font::DEFAULT
    }
}

/// The font in its semibold weight, for titles and names.
pub fn semibold() -> Font {
    Font {
        weight: font::Weight::Semibold,
        ..font()
    }
}

/// A card: white, a hairline border, rounded, and a shadow just strong enough to lift it.
pub fn card(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(CARD)),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(CARD_RADIUS),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.06),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..container::Style::default()
    }
}

/// The page itself.
pub fn page(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(PAGE)),
        ..container::Style::default()
    }
}

/// Windows' warning bar: amber tint, amber hairline.
pub fn warning_bar(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(TEXT),
        background: Some(Background::Color(WARNING_TINT)),
        border: Border {
            color: WARNING_BORDER,
            width: 1.0,
            radius: Radius::from(CARD_RADIUS),
        },
        ..container::Style::default()
    }
}

/// A one-pixel line between rows in a card.
pub fn divider(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(DIVIDER)),
        ..container::Style::default()
    }
}

/// The accent strip beside the selected sidebar item.
pub fn selected_strip(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(ACCENT)),
        border: Border {
            radius: Radius::from(1.5),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The main action: accent blue, white text.
pub fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => ACCENT_HOVER,
        button::Status::Pressed => ACCENT_PRESSED,
        button::Status::Disabled => SUBTLE_PRESSED,
        button::Status::Active => ACCENT,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.1),
            width: 1.0,
            radius: Radius::from(CONTROL_RADIUS),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A button that deletes: Windows' red, white text.
pub fn danger_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => hex(0xB02A1C),
        button::Status::Pressed => hex(0x9E2618),
        button::Status::Disabled => SUBTLE_PRESSED,
        button::Status::Active => DANGER,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: Color::WHITE,
        border: Border {
            radius: Radius::from(CONTROL_RADIUS),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// Any other button: white with a hairline, grey under the pointer.
pub fn standard_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => hex(0xF9F9F9),
        button::Status::Pressed => hex(0xF5F5F5),
        button::Status::Active | button::Status::Disabled => CARD,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if status == button::Status::Disabled {
            TEXT_SECONDARY
        } else {
            TEXT
        },
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(CONTROL_RADIUS),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.04),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 1.0,
        },
        snap: true,
    }
}

/// A sidebar item, or a row that opens something: no face until the pointer is on it.
pub fn subtle_button(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = match status {
            button::Status::Pressed => Some(SUBTLE_PRESSED),
            button::Status::Hovered => Some(SUBTLE_HOVER),
            _ if selected => Some(SUBTLE_HOVER),
            _ => None,
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: TEXT,
            border: Border {
                radius: Radius::from(CONTROL_RADIUS),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

/// A row inside a card that opens something: a faint tint under the pointer, square, so it
/// stays inside the card's rounded corners.
pub fn row_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Pressed => Some(hex(0xF0F0F0)),
        button::Status::Hovered => Some(hex(0xF7F7F7)),
        _ => None,
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: TEXT,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A field's bottom edge, darker, as Windows 11 draws its text boxes.
const FIELD_BORDER: Color = hex(0xD1D1D1);

/// A field's border: the accent when focused, else the hairline.
fn field_border(focused: bool) -> Border {
    Border {
        color: if focused { ACCENT } else { FIELD_BORDER },
        width: if focused { 1.5 } else { 1.0 },
        radius: Radius::from(CONTROL_RADIUS),
    }
}

/// A text box: white, a hairline, the accent around it while typing.
pub fn text_field(_theme: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(if status == text_input::Status::Hovered {
            hex(0xFDFDFD)
        } else {
            CARD
        }),
        border: field_border(focused),
        icon: TEXT_SECONDARY,
        placeholder: hex(0x8A8A8A),
        value: TEXT,
        selection: Color { a: 0.3, ..ACCENT },
    }
}

/// A text box of several lines, as text_field.
pub fn text_area(_theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let focused = matches!(status, text_editor::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(if status == text_editor::Status::Hovered {
            hex(0xFDFDFD)
        } else {
            CARD
        }),
        border: field_border(focused),
        placeholder: hex(0x8A8A8A),
        value: TEXT,
        selection: Color { a: 0.3, ..ACCENT },
    }
}

/// A check box: white with a hairline, the accent with a white tick when ticked.
pub fn check_box(_theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let (checked, hovered) = match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Disabled { is_checked } => (is_checked, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true),
    };
    let face = match (checked, hovered) {
        (true, false) => ACCENT,
        (true, true) => ACCENT_HOVER,
        (false, false) => CARD,
        (false, true) => hex(0xF9F9F9),
    };
    checkbox::Style {
        background: Background::Color(face),
        icon_color: Color::WHITE,
        border: Border {
            color: if checked { face } else { FIELD_BORDER },
            width: 1.0,
            radius: Radius::from(CONTROL_RADIUS),
        },
        text_color: Some(TEXT),
    }
}

/// A list to pick from, looking like a text box with its arrow.
pub fn pick_field(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: TEXT,
        placeholder_color: hex(0x8A8A8A),
        handle_color: TEXT_SECONDARY,
        background: Background::Color(if status == pick_list::Status::Hovered {
            hex(0xF9F9F9)
        } else {
            CARD
        }),
        border: field_border(matches!(status, pick_list::Status::Opened { .. })),
    }
}

/// A list's open menu: a white card, the picked entry on the accent's tint.
pub fn pick_menu(_theme: &Theme) -> menu::Style {
    menu::Style {
        background: Background::Color(CARD),
        border: Border {
            color: CARD_BORDER,
            width: 1.0,
            radius: Radius::from(CONTROL_RADIUS),
        },
        text_color: TEXT,
        selected_text_color: TEXT,
        selected_background: Background::Color(hex(0xE5EFF8)),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.14),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

/// A progress bar: the accent on a grey track, rounded.
pub fn progress(_theme: &Theme) -> iced::widget::progress_bar::Style {
    iced::widget::progress_bar::Style {
        background: Background::Color(DIVIDER),
        bar: Background::Color(ACCENT),
        border: Border {
            radius: Radius::from(3.0),
            ..Border::default()
        },
    }
}

/// A small label on the accent's tint, like "Suggested".
pub fn badge(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(hex(0xE5EFF8))),
        border: Border {
            radius: Radius::from(10.0),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A small pill in a health's colours, like a post's "Failed" or a search's "Paused".
pub fn chip(health: Health) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        background: Some(Background::Color(health_colors(health).1)),
        text_color: Some(health_colors(health).0),
        border: Border {
            radius: Radius::from(10.0),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Secondary text.
pub fn secondary(_theme: &Theme) -> text::Style {
    text::Style {
        color: Some(TEXT_SECONDARY),
    }
}

/// How a health reads: its colour, and the tint behind it.
fn health_colors(health: Health) -> (Color, Color) {
    match health {
        Health::Ready => (SUCCESS, SUCCESS_TINT),
        Health::Attention => (WARNING, WARNING_TINT),
        Health::Neutral => (NEUTRAL, NEUTRAL_TINT),
    }
}

/// Text in a health's colour.
pub fn health_text(health: Health) -> impl Fn(&Theme) -> text::Style {
    move |_theme| text::Style {
        color: Some(health_colors(health).0),
    }
}

/// The size of a status row's round mark.
const MARK_SIZE: f32 = 28.0;
/// The size of a status line's dot.
const DOT_SIZE: f32 = 8.0;

/// A status row's mark: its glyph in the health's colour on a round tint, so the meaning is in
/// the shape too, not only the colour.
pub fn mark<'a, Message: 'a>(health: Health) -> Element<'a, Message> {
    let glyph = match health {
        Health::Ready => "✓",
        Health::Attention => "!",
        Health::Neutral => "•",
    };
    let (color, tint) = health_colors(health);
    container(text(glyph).font(semibold()).size(BODY_SIZE).color(color))
        .center(MARK_SIZE)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(tint)),
            border: Border {
                radius: Radius::from(MARK_SIZE / 2.0),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}

/// The status line's dot in a health's colour.
pub fn dot<'a, Message: 'a>(health: Health) -> Element<'a, Message> {
    let color = health_colors(health).0;
    container(iced::widget::space())
        .width(DOT_SIZE)
        .height(DOT_SIZE)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: Radius::from(DOT_SIZE / 2.0),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}
