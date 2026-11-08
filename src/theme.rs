//! Palette, widget styles and inline icons. Warm, quiet, one amber accent.

use iced::widget::{button, container, pick_list, slider, svg, text_input, toggler};
use iced::{Background, Border, Color, Element, Shadow, Theme, Vector, border, color, theme};

pub const SPACE_XS: f32 = 6.0;
pub const SPACE_S: f32 = 10.0;
pub const SPACE_M: f32 = 16.0;
pub const RADIUS: f32 = 10.0;
pub const SIDEBAR_WIDTH: f32 = 372.0;

#[derive(Clone, Copy)]
pub struct Colors {
    pub bg: Color,
    pub sidebar: Color,
    pub card: Color,
    pub field: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub on_accent: Color,
    pub warn: Color,
    pub danger: Color,
    pub shadow: Color,
}

const LIGHT: Colors = Colors {
    bg: color!(0xEFEAE3),
    sidebar: color!(0xF7F4EF),
    card: color!(0xFFFFFF),
    field: color!(0xF1EDE6),
    border: color!(0xE2DBD1),
    text: color!(0x2B2622),
    muted: color!(0x7A716A),
    faint: color!(0xB8AFA6),
    accent: color!(0xD98E3F),
    accent_soft: color!(0xF7E6D2),
    on_accent: color!(0xFFFFFF),
    warn: color!(0xC98A1E),
    danger: color!(0xC9553D),
    shadow: Color { r: 0.16, g: 0.12, b: 0.08, a: 0.18 },
};

const DARK: Colors = Colors {
    bg: color!(0x171513),
    sidebar: color!(0x201D1A),
    card: color!(0x2A2622),
    field: color!(0x332E29),
    border: color!(0x3B3530),
    text: color!(0xEDE6DD),
    muted: color!(0x9C938A),
    faint: color!(0x5E564F),
    accent: color!(0xE49A4C),
    accent_soft: color!(0x4A3521),
    on_accent: color!(0x1E1A16),
    warn: color!(0xE0A63A),
    danger: color!(0xE26B52),
    shadow: Color { r: 0.0, g: 0.0, b: 0.0, a: 0.5 },
};

pub fn colors(theme: &Theme) -> Colors {
    if theme.extended_palette().is_dark { DARK } else { LIGHT }
}

pub fn make_theme(dark: bool) -> Theme {
    let c = if dark { DARK } else { LIGHT };
    Theme::custom(
        if dark { "Badger Dark" } else { "Badger" },
        theme::Palette { background: c.sidebar, text: c.text, primary: c.accent, success: color!(0x5B9C6A), warning: c.warn, danger: c.danger },
    )
}

fn rounded(radius: f32, color: Color, width: f32) -> Border {
    Border { color, width, radius: border::Radius::new(radius) }
}

pub fn sidebar(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style { background: Some(c.sidebar.into()), text_color: Some(c.text), ..Default::default() }
}

pub fn workspace(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style { background: Some(c.bg.into()), text_color: Some(c.text), ..Default::default() }
}

pub fn card(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.card.into()),
        border: rounded(RADIUS, c.border, 1.0),
        text_color: Some(c.text),
        ..Default::default()
    }
}

pub fn floating(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style {
        background: Some(c.card.into()),
        border: rounded(RADIUS + 2.0, c.border, 1.0),
        shadow: Shadow { color: c.shadow, offset: Vector::new(0.0, 6.0), blur_radius: 18.0 },
        text_color: Some(c.text),
        ..Default::default()
    }
}

pub fn chip(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style { background: Some(c.field.into()), border: rounded(999.0, Color::TRANSPARENT, 0.0), text_color: Some(c.muted), ..Default::default() }
}

pub fn chip_accent(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style { background: Some(c.accent_soft.into()), border: rounded(999.0, Color::TRANSPARENT, 0.0), text_color: Some(c.accent), ..Default::default() }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let bg = match status {
        button::Status::Hovered => Color { a: 0.9, ..c.accent },
        button::Status::Pressed => Color { r: c.accent.r * 0.85, g: c.accent.g * 0.85, b: c.accent.b * 0.85, a: 1.0 },
        button::Status::Disabled => Color { a: 0.35, ..c.accent },
        button::Status::Active => c.accent,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: c.on_accent,
        border: rounded(RADIUS, Color::TRANSPARENT, 0.0),
        shadow: Shadow { color: Color { a: 0.25, ..c.accent }, offset: Vector::new(0.0, 2.0), blur_radius: 6.0 },
        ..Default::default()
    }
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let bg = match status {
        button::Status::Hovered => c.field,
        button::Status::Pressed => c.border,
        _ => c.card,
    };
    button::Style {
        background: Some(bg.into()),
        text_color: if matches!(status, button::Status::Disabled) { c.faint } else { c.text },
        border: rounded(RADIUS, c.border, 1.0),
        ..Default::default()
    }
}

pub fn ghost(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    let bg = match status {
        button::Status::Hovered => Some(Background::Color(c.field)),
        button::Status::Pressed => Some(Background::Color(c.border)),
        _ => None,
    };
    button::Style {
        background: bg,
        text_color: if matches!(status, button::Status::Disabled) { c.faint } else { c.muted },
        border: rounded(8.0, Color::TRANSPARENT, 0.0),
        ..Default::default()
    }
}

pub fn link(theme: &Theme, status: button::Status) -> button::Style {
    let c = colors(theme);
    button::Style {
        background: None,
        text_color: if matches!(status, button::Status::Hovered) { c.text } else { c.accent },
        border: rounded(0.0, Color::TRANSPARENT, 0.0),
        ..Default::default()
    }
}

pub fn segment(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let (bg, fg) = match (selected, status) {
            (true, _) => (Some(c.card), c.text),
            (false, button::Status::Hovered) => (Some(Color { a: 0.5, ..c.card }), c.text),
            (false, _) => (None, c.muted),
        };
        button::Style {
            background: bg.map(Background::Color),
            text_color: fg,
            border: rounded(7.0, Color::TRANSPARENT, 0.0),
            shadow: if selected { Shadow { color: Color { a: 0.12, ..c.shadow }, offset: Vector::new(0.0, 1.0), blur_radius: 3.0 } } else { Shadow::default() },
            ..Default::default()
        }
    }
}

pub fn swatch(rgb: [f32; 3], selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let c = colors(theme);
        let ring = if selected { c.accent } else if matches!(status, button::Status::Hovered) { c.faint } else { c.border };
        button::Style {
            background: Some(Color::from_rgb(rgb[0], rgb[1], rgb[2]).into()),
            text_color: c.text,
            border: rounded(999.0, ring, if selected { 2.0 } else { 1.0 }),
            ..Default::default()
        }
    }
}

pub fn segment_track(theme: &Theme) -> container::Style {
    let c = colors(theme);
    container::Style { background: Some(c.field.into()), border: rounded(9.0, Color::TRANSPARENT, 0.0), ..Default::default() }
}

pub fn slider_style(theme: &Theme, status: slider::Status) -> slider::Style {
    let c = colors(theme);
    let handle_bg = match status {
        slider::Status::Dragged => c.accent,
        _ => c.card,
    };
    slider::Style {
        rail: slider::Rail { backgrounds: (c.accent.into(), c.border.into()), width: 4.0, border: rounded(2.0, Color::TRANSPARENT, 0.0) },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 8.0 },
            background: handle_bg.into(),
            border_width: 1.5,
            border_color: c.accent,
        },
    }
}

pub fn field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let c = colors(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => c.accent,
        text_input::Status::Hovered => c.faint,
        _ => c.border,
    };
    text_input::Style {
        background: c.field.into(),
        border: rounded(8.0, border_color, 1.0),
        icon: c.muted,
        placeholder: c.faint,
        value: c.text,
        selection: c.accent_soft,
    }
}

pub fn picker(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let c = colors(theme);
    pick_list::Style {
        text_color: c.text,
        placeholder_color: c.faint,
        handle_color: c.muted,
        background: c.field.into(),
        border: rounded(8.0, if matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened { .. }) { c.faint } else { c.border }, 1.0),
    }
}

pub fn switch(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let c = colors(theme);
    let active = matches!(status, toggler::Status::Active { is_toggled: true } | toggler::Status::Hovered { is_toggled: true });
    toggler::Style {
        background: if active { c.accent } else { c.border }.into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: c.card.into(),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.2,
    }
}

// ---- Icons (Lucide, ISC licence) ----

pub const EXPORT: &str = r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>"#;
pub const PRINT: &str = r#"<polyline points="6 9 6 2 18 2 18 9"/><path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2"/><rect width="12" height="8" x="6" y="14"/>"#;
pub const FIT: &str = r#"<path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/>"#;
pub const TRASH: &str = r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>"#;
pub const PLUS: &str = r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#;
pub const SUN: &str = r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#;
pub const MOON: &str = r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#;
pub const RULER: &str = r#"<path d="M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z"/><path d="m14.5 12.5 2-2"/><path d="m11.5 9.5 2-2"/><path d="m8.5 6.5 2-2"/><path d="m17.5 15.5 2-2"/>"#;
pub const CHEVRON_UP: &str = r#"<path d="m18 15-6-6-6 6"/>"#;
pub const CHEVRON_DOWN: &str = r#"<path d="m6 9 6 6 6-6"/>"#;
pub const CHEVRON_L: &str = r#"<path d="m15 18-6-6 6-6"/>"#;
pub const CHEVRON_R: &str = r#"<path d="m9 18 6-6-6-6"/>"#;
pub const CLOSE: &str = r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#;
pub const BOOKMARK: &str = r#"<path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"/>"#;
pub const UNDO: &str = r#"<path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"/>"#;

pub fn icon<'a, M: 'a>(body: &'static str, size: f32, tone: fn(&Colors) -> Color) -> Element<'a, M> {
    let data = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"##
    );
    svg(svg::Handle::from_memory(data.into_bytes()))
        .width(size)
        .height(size)
        .style(move |theme: &Theme, _| svg::Style { color: Some(tone(&colors(theme))) })
        .into()
}
