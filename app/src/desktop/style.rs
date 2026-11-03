//! The window's look, after Windows 11's (Fluent): a grey page, cards with a hairline border and
//! a soft shadow, Windows' blue as the accent, and status marks on soft tinted pills; light or
//! dark as Windows is set (theme). Every colour and size the screens use is named here, so they
//! stay one design.

use iced::border::Radius;
use iced::overlay::menu;
use iced::theme::Mode;
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

/// Every colour of one look, light or dark.
pub struct Colors {
    /// The page behind the cards (Windows 11's Mica).
    pub page: Color,
    /// A card's face, and a field's or a plain button's.
    pub card: Color,
    /// A card's hairline border.
    pub card_border: Color,
    /// The line between rows inside a card, and a progress bar's track.
    pub divider: Color,
    /// Body text.
    pub text: Color,
    /// Secondary text: notes, captions, paths.
    pub text_secondary: Color,
    /// Windows' accent blue.
    pub accent: Color,
    /// The accent under the pointer.
    accent_hover: Color,
    /// The accent pressed.
    accent_pressed: Color,
    /// Text on the accent.
    on_accent: Color,
    /// The accent's tint, behind a badge or a picked entry.
    accent_tint: Color,
    /// A plain button's face under the pointer, and the selected sidebar item.
    subtle_hover: Color,
    /// A plain button pressed, and a disabled main button.
    subtle_pressed: Color,
    /// A card's button or field under the pointer.
    control_hover: Color,
    /// A card's button pressed.
    control_pressed: Color,
    /// A field's border, darker than a card's, as Windows 11 draws its text boxes.
    field_border: Color,
    /// A field's text before anything is typed.
    placeholder: Color,
    /// How much a card's shadow darkens.
    shadow: f32,
    /// Ready: green on a green tint.
    success: Color,
    success_tint: Color,
    /// Needs attention: amber on an amber tint, as Windows' warning bar.
    warning: Color,
    warning_tint: Color,
    warning_border: Color,
    /// Something failed.
    danger: Color,
    /// A note: grey on a grey tint.
    neutral: Color,
    neutral_tint: Color,
}

/// Windows 11's light look.
pub const LIGHT: Colors = Colors {
    page: hex(0xF3F3F3),
    card: hex(0xFFFFFF),
    card_border: hex(0xE5E5E5),
    divider: hex(0xEDEDED),
    text: hex(0x1A1A1A),
    text_secondary: hex(0x5F5F5F),
    accent: hex(0x005FB8),
    accent_hover: hex(0x196EBF),
    accent_pressed: hex(0x3C82C9),
    on_accent: Color::WHITE,
    accent_tint: hex(0xE5EFF8),
    subtle_hover: hex(0xEAEAEA),
    subtle_pressed: hex(0xE0E0E0),
    control_hover: hex(0xF9F9F9),
    control_pressed: hex(0xF0F0F0),
    field_border: hex(0xD1D1D1),
    placeholder: hex(0x8A8A8A),
    shadow: 0.06,
    success: hex(0x0F7B0F),
    success_tint: hex(0xDFF6DD),
    warning: hex(0x9D5D00),
    warning_tint: hex(0xFFF4CE),
    warning_border: hex(0xF2D27A),
    danger: hex(0xC42B1C),
    neutral: hex(0x616161),
    neutral_tint: hex(0xF0F0F0),
};

/// Windows 11's dark look.
pub const DARK: Colors = Colors {
    page: hex(0x202020),
    card: hex(0x2B2B2B),
    card_border: hex(0x3A3A3A),
    divider: hex(0x353535),
    text: hex(0xFFFFFF),
    text_secondary: hex(0xC5C5C5),
    accent: hex(0x60CDFF),
    accent_hover: hex(0x5BBDEA),
    accent_pressed: hex(0x56AED6),
    on_accent: hex(0x000000),
    accent_tint: hex(0x1F3A4D),
    subtle_hover: hex(0x2D2D2D),
    subtle_pressed: hex(0x292929),
    control_hover: hex(0x323232),
    control_pressed: hex(0x272727),
    field_border: hex(0x4D4D4D),
    placeholder: hex(0x9E9E9E),
    shadow: 0.3,
    success: hex(0x6CCB5F),
    success_tint: hex(0x2A3A27),
    warning: hex(0xFCE100),
    warning_tint: hex(0x433519),
    warning_border: hex(0x5C4A1A),
    danger: hex(0xC42B1C),
    neutral: hex(0xB4B4B4),
    neutral_tint: hex(0x353535),
};

/// The look a theme is: dark or light.
pub fn colors(theme: &Theme) -> &'static Colors {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}

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

/// The window's theme for Windows' light or dark mode (light when it doesn't say): its palette,
/// which iced's own widgets (scrollbars, focus) take colours from.
pub fn theme(mode: Mode) -> Theme {
    let (name, c) = match mode {
        Mode::Dark => ("JobFinder dark", &DARK),
        Mode::Light | Mode::None => ("JobFinder", &LIGHT),
    };
    Theme::custom(
        name,
        iced::theme::Palette {
            background: c.page,
            text: c.text,
            primary: c.accent,
            success: c.success,
            warning: c.warning,
            danger: c.danger,
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

/// The bell beside the app's name: Segoe Fluent Icons' Ringer on Windows 11, the emoji's bell
/// elsewhere.
pub const BELL: &str = if cfg!(windows) { "\u{EA8F}" } else { "\u{1F514}" };

/// The font the bell is drawn in: Windows 11's icon font there, else the system's own.
pub fn icon_font() -> Font {
    if cfg!(windows) {
        Font::with_name("Segoe Fluent Icons")
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
pub fn card(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        text_color: Some(c.text),
        background: Some(Background::Color(c.card)),
        border: Border {
            color: c.card_border,
            width: 1.0,
            radius: Radius::from(CARD_RADIUS),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, c.shadow),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..container::Style::default()
    }
}

/// The page itself.
pub fn page(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        text_color: Some(c.text),
        background: Some(Background::Color(c.page)),
        ..container::Style::default()
    }
}

/// Windows' warning bar: amber tint, amber hairline.
pub fn warning_bar(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        text_color: Some(c.text),
        background: Some(Background::Color(c.warning_tint)),
        border: Border {
            color: c.warning_border,
            width: 1.0,
            radius: Radius::from(CARD_RADIUS),
        },
        ..container::Style::default()
    }
}

/// A one-pixel line between rows in a card.
pub fn divider(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(c.divider)),
        ..container::Style::default()
    }
}

/// The accent strip beside the selected sidebar item.
pub fn selected_strip(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(c.accent)),
        border: Border {
            radius: Radius::from(1.5),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The main action: accent blue, white text.
pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered => c.accent_hover,
        button::Status::Pressed => c.accent_pressed,
        button::Status::Disabled => c.subtle_pressed,
        button::Status::Active => c.accent,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if status == button::Status::Disabled {
            c.text_secondary
        } else {
            c.on_accent
        },
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
pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered => hex(0xB02A1C),
        button::Status::Pressed => hex(0x9E2618),
        button::Status::Disabled => c.subtle_pressed,
        button::Status::Active => c.danger,
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
pub fn standard_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered => c.control_hover,
        button::Status::Pressed => c.control_pressed,
        button::Status::Active | button::Status::Disabled => c.card,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if status == button::Status::Disabled {
            c.text_secondary
        } else {
            c.text
        },
        border: Border {
            color: c.card_border,
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
    move |theme, status| {
        let c = colors(theme);
        let background = match status {
            button::Status::Pressed => Some(c.subtle_pressed),
            button::Status::Hovered => Some(c.subtle_hover),
            _ if selected => Some(c.subtle_hover),
            _ => None,
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: c.text,
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
pub fn row_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Pressed => Some(c.control_pressed),
        button::Status::Hovered => Some(c.control_hover),
        _ => None,
    };
    button::Style {
        background: background.map(Background::Color),
        text_color: c.text,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A web address that opens in the browser: accent text, no face, the darker accent under the
/// pointer.
pub fn link_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let text_color = match status {
        button::Status::Pressed => c.accent_pressed,
        button::Status::Hovered => c.accent_hover,
        _ => c.accent,
    };
    button::Style {
        background: None,
        text_color,
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A field's border: the accent when focused, else the hairline.
fn field_border(c: &Colors, focused: bool) -> Border {
    Border {
        color: if focused { c.accent } else { c.field_border },
        width: if focused { 1.5 } else { 1.0 },
        radius: Radius::from(CONTROL_RADIUS),
    }
}

/// A text box: white, a hairline, the accent around it while typing.
pub fn text_field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = colors(theme);
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: Background::Color(if status == text_input::Status::Hovered {
            c.control_hover
        } else {
            c.card
        }),
        border: field_border(c, focused),
        icon: c.text_secondary,
        placeholder: c.placeholder,
        value: c.text,
        selection: Color { a: 0.3, ..c.accent },
    }
}

/// A text box of several lines, as text_field.
pub fn text_area(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let c = colors(theme);
    let focused = matches!(status, text_editor::Status::Focused { .. });
    text_editor::Style {
        background: Background::Color(if status == text_editor::Status::Hovered {
            c.control_hover
        } else {
            c.card
        }),
        border: field_border(c, focused),
        placeholder: c.placeholder,
        value: c.text,
        selection: Color { a: 0.3, ..c.accent },
    }
}

/// A check box: white with a hairline, the accent with a white tick when ticked.
pub fn check_box(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let c = colors(theme);
    let (checked, hovered) = match status {
        checkbox::Status::Active { is_checked } | checkbox::Status::Disabled { is_checked } => (is_checked, false),
        checkbox::Status::Hovered { is_checked } => (is_checked, true),
    };
    let face = match (checked, hovered) {
        (true, false) => c.accent,
        (true, true) => c.accent_hover,
        (false, false) => c.card,
        (false, true) => c.control_hover,
    };
    checkbox::Style {
        background: Background::Color(face),
        icon_color: c.on_accent,
        border: Border {
            color: if checked { face } else { c.field_border },
            width: 1.0,
            radius: Radius::from(CONTROL_RADIUS),
        },
        text_color: Some(c.text),
    }
}

/// A list to pick from, looking like a text box with its arrow.
pub fn pick_field(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let c = colors(theme);
    pick_list::Style {
        text_color: c.text,
        placeholder_color: c.placeholder,
        handle_color: c.text_secondary,
        background: Background::Color(if status == pick_list::Status::Hovered {
            c.control_hover
        } else {
            c.card
        }),
        border: field_border(c, matches!(status, pick_list::Status::Opened { .. })),
    }
}

/// A list's open menu: a white card, the picked entry on the accent's tint.
pub fn pick_menu(theme: &Theme) -> menu::Style {
    let c = colors(theme);
    menu::Style {
        background: Background::Color(c.card),
        border: Border {
            color: c.card_border,
            width: 1.0,
            radius: Radius::from(CONTROL_RADIUS),
        },
        text_color: c.text,
        selected_text_color: c.text,
        selected_background: Background::Color(c.accent_tint),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.14),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

/// A progress bar: the accent on a grey track, rounded.
pub fn progress(theme: &Theme) -> iced::widget::progress_bar::Style {
    let c = colors(theme);
    iced::widget::progress_bar::Style {
        background: Background::Color(c.divider),
        bar: Background::Color(c.accent),
        border: Border {
            radius: Radius::from(3.0),
            ..Border::default()
        },
    }
}

/// A small label on the accent's tint, like "Suggested".
pub fn badge(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(Background::Color(c.accent_tint)),
        border: Border {
            radius: Radius::from(10.0),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A small pill in a health's colours, like a post's "Failed" or a search's "Paused".
pub fn chip(health: Health) -> impl Fn(&Theme) -> container::Style {
    move |theme| container::Style {
        background: Some(Background::Color(health_colors(colors(theme), health).1)),
        text_color: Some(health_colors(colors(theme), health).0),
        border: Border {
            radius: Radius::from(10.0),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// Text in the accent colour.
pub fn accent_text(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(colors(theme).accent),
    }
}

/// Secondary text.
pub fn secondary(theme: &Theme) -> text::Style {
    let c = colors(theme);
    text::Style {
        color: Some(c.text_secondary),
    }
}

/// How a health reads: its colour, and the tint behind it.
fn health_colors(c: &Colors, health: Health) -> (Color, Color) {
    match health {
        Health::Ready => (c.success, c.success_tint),
        Health::Attention => (c.warning, c.warning_tint),
        Health::Neutral => (c.neutral, c.neutral_tint),
    }
}

/// Text in a health's colour.
pub fn health_text(health: Health) -> impl Fn(&Theme) -> text::Style {
    move |theme| text::Style {
        color: Some(health_colors(colors(theme), health).0),
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
    container(text(glyph).font(semibold()).size(BODY_SIZE).style(health_text(health)))
        .center(MARK_SIZE)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(health_colors(colors(theme), health).1)),
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
    container(iced::widget::space())
        .width(DOT_SIZE)
        .height(DOT_SIZE)
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(health_colors(colors(theme), health).0)),
            border: Border {
                radius: Radius::from(DOT_SIZE / 2.0),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
}
