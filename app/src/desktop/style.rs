//! The window's look, the same as JobFinder's web page: oat paper with warm ink in light mode, a
//! warm evening brown in dark mode, Newsreader for titles and Figtree for the rest, sage green
//! as the accent, rust for links, ink for the main button, and status marks on soft tinted pills;
//! light or dark as the system is set (theme). Every colour, size and font the screens use is
//! named here, so they stay one design.

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
    /// The page behind the cards: oat paper.
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
    /// The accent: sage green, as the web page's drawings and good scores.
    pub accent: Color,
    /// The accent under the pointer.
    accent_hover: Color,
    /// Text on the accent.
    on_accent: Color,
    /// The accent's tint, behind a badge or a picked entry.
    accent_tint: Color,
    /// The main button's face: ink, as the web page's Download button.
    primary: Color,
    /// The main button under the pointer.
    primary_hover: Color,
    /// The main button pressed.
    primary_pressed: Color,
    /// Text on the main button.
    on_primary: Color,
    /// A web address: rust, as the web page's links.
    link: Color,
    /// A web address under the pointer.
    link_hover: Color,
    /// A plain button's face under the pointer, and the selected sidebar item.
    subtle_hover: Color,
    /// A plain button pressed, and a disabled main button.
    subtle_pressed: Color,
    /// A card's button or field under the pointer.
    control_hover: Color,
    /// A card's button pressed.
    control_pressed: Color,
    /// A field's border, darker than a card's.
    field_border: Color,
    /// A field's text before anything is typed.
    placeholder: Color,
    /// How much a card's shadow darkens.
    shadow: f32,
    /// Ready: green on a green tint.
    success: Color,
    success_tint: Color,
    /// Needs attention: amber on an amber tint.
    warning: Color,
    warning_tint: Color,
    warning_border: Color,
    /// Something failed, and the face of a button that deletes.
    danger: Color,
    /// A button that deletes, under the pointer.
    danger_hover: Color,
    /// A button that deletes, pressed.
    danger_pressed: Color,
    /// A note: grey on a grey tint.
    neutral: Color,
    neutral_tint: Color,
}

/// The light look: morning paper.
pub const LIGHT: Colors = Colors {
    page: hex(0xF5EFE4),
    card: hex(0xFCF9F3),
    card_border: hex(0xE2D7C4),
    divider: hex(0xEBE2D2),
    text: hex(0x2A231C),
    text_secondary: hex(0x6F6456),
    accent: hex(0x4F6D4D),
    accent_hover: hex(0x5D7A5A),
    on_accent: hex(0xFCF9F3),
    accent_tint: hex(0xE1E9DC),
    primary: hex(0x2A231C),
    primary_hover: hex(0x3D342A),
    primary_pressed: hex(0x4D4236),
    on_primary: hex(0xF5EFE4),
    link: hex(0x8A4A2F),
    link_hover: hex(0x6A361F),
    subtle_hover: hex(0xECE4D6),
    subtle_pressed: hex(0xE3D9C8),
    control_hover: hex(0xF7F1E7),
    control_pressed: hex(0xEFE7DA),
    field_border: hex(0xD4C7B2),
    placeholder: hex(0x9A8E7E),
    shadow: 0.07,
    success: hex(0x4F6D4D),
    success_tint: hex(0xE1E9DC),
    warning: hex(0x8A5A12),
    warning_tint: hex(0xF8EAD0),
    warning_border: hex(0xE6CD98),
    danger: hex(0xB23A26),
    danger_hover: hex(0x9E3221),
    danger_pressed: hex(0x8A2B1C),
    neutral: hex(0x6F6456),
    neutral_tint: hex(0xECE4D6),
};

/// The dark look: a warm evening brown.
pub const DARK: Colors = Colors {
    page: hex(0x221B16),
    card: hex(0x2D241E),
    card_border: hex(0x43362C),
    divider: hex(0x3A2F27),
    text: hex(0xF1E6D6),
    text_secondary: hex(0xB8A792),
    accent: hex(0x9DBF9A),
    accent_hover: hex(0xAFCCAC),
    on_accent: hex(0x221B16),
    accent_tint: hex(0x2F3A2C),
    primary: hex(0xF1E6D6),
    primary_hover: hex(0xE4D6C2),
    primary_pressed: hex(0xD6C6B0),
    on_primary: hex(0x221B16),
    link: hex(0xE6A46A),
    link_hover: hex(0xEEB27C),
    subtle_hover: hex(0x342A23),
    subtle_pressed: hex(0x2F261F),
    control_hover: hex(0x352B24),
    control_pressed: hex(0x2A221C),
    field_border: hex(0x57473A),
    placeholder: hex(0x8F8070),
    shadow: 0.3,
    success: hex(0x9DBF9A),
    success_tint: hex(0x2F3A2C),
    warning: hex(0xE9B45A),
    warning_tint: hex(0x45351C),
    warning_border: hex(0x5F4A26),
    danger: hex(0xB8452F),
    danger_hover: hex(0xA33D29),
    danger_pressed: hex(0x8F3524),
    neutral: hex(0xB8A792),
    neutral_tint: hex(0x3A2F27),
};

/// The look a theme is: dark or light.
pub fn colors(theme: &Theme) -> &'static Colors {
    if theme.extended_palette().is_dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// Corner radius of cards and the warning bar, as the web page's cards.
pub const CARD_RADIUS: f32 = 12.0;
/// Corner radius of buttons, fields and sidebar items.
const CONTROL_RADIUS: f32 = 8.0;
/// Corner radius of the main button: a pill, as the web page's Download button (more than half
/// any button's height).
const PILL_RADIUS: f32 = 999.0;

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

/// The window's theme for the system's light or dark mode (light when it doesn't say): its palette,
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

/// Figtree, the web page's sans-serif, for everything but titles (SIL Open Font License,
/// assets/fonts/OFL-Figtree.txt).
const FIGTREE: &[u8] = include_bytes!("../../../assets/fonts/Figtree-Regular.ttf");
/// Figtree's semibold, for names and buttons.
const FIGTREE_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Figtree-SemiBold.ttf");
/// Newsreader, the web page's serif, for titles (SIL Open Font License,
/// assets/fonts/OFL-Newsreader.txt).
const NEWSREADER: &[u8] = include_bytes!("../../../assets/fonts/Newsreader-Regular.ttf");

/// The fonts built into the program, for the window to load at start.
pub const FONTS: [&[u8]; 3] = [FIGTREE, FIGTREE_SEMIBOLD, NEWSREADER];

/// The font everything is written in: Figtree.
pub fn font() -> Font {
    Font::with_name("Figtree")
}

/// The font titles are written in: Newsreader.
pub fn title_font() -> Font {
    Font::with_name("Newsreader")
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

/// A card: paper, a hairline border, rounded, and a shadow just strong enough to lift it.
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

/// The warning bar: amber tint, amber hairline.
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

/// The main action: an ink pill with paper text.
pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered => c.primary_hover,
        button::Status::Pressed => c.primary_pressed,
        button::Status::Disabled => c.subtle_pressed,
        button::Status::Active => c.primary,
    };
    button::Style {
        background: Some(Background::Color(background)),
        text_color: if status == button::Status::Disabled {
            c.text_secondary
        } else {
            c.on_primary
        },
        border: Border {
            radius: Radius::from(PILL_RADIUS),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

/// A button that deletes: red, white text.
pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let background = match status {
        button::Status::Hovered => c.danger_hover,
        button::Status::Pressed => c.danger_pressed,
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

/// Any other button: paper with a hairline, darker under the pointer.
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

/// A web address that opens in the browser: rust text, no face, darker under the pointer.
pub fn link_button(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let text_color = match status {
        button::Status::Pressed | button::Status::Hovered => c.link_hover,
        _ => c.link,
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

/// A text box: paper, a hairline, the accent around it while typing.
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

/// A check box: paper with a hairline, the accent with a paper tick when ticked.
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

/// A list's open menu: a paper card, the picked entry on the accent's tint.
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
