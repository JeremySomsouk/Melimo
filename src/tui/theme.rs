use ratatui::{
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType},
};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
struct Palette {
    background: Color,
    foreground: Color,
    accent: Color,
    muted: Color,
    warning: Color,
}

fn palette() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(|| {
        let mode = std::env::var("MELIMO_THEME").unwrap_or_default();
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        match mode.as_str() {
            _ if no_color || mode == "mono" => Palette {
                background: Color::Reset,
                foreground: Color::Reset,
                accent: Color::Reset,
                muted: Color::Reset,
                warning: Color::Reset,
            },
            "light" => Palette {
                background: Color::Rgb(250, 248, 253),
                foreground: Color::Rgb(40, 33, 52),
                accent: Color::Rgb(105, 55, 160),
                muted: Color::Rgb(103, 94, 116),
                warning: Color::Rgb(160, 45, 55),
            },
            _ => Palette {
                background: Color::Rgb(24, 22, 32),
                foreground: Color::Rgb(237, 231, 245),
                accent: Color::Rgb(193, 157, 244),
                muted: Color::Rgb(160, 151, 177),
                warning: Color::Rgb(255, 159, 154),
            },
        }
    })
}

pub fn base() -> Style {
    Style::default()
        .fg(palette().foreground)
        .bg(palette().background)
}

pub fn title() -> Style {
    base().fg(palette().accent).add_modifier(Modifier::BOLD)
}

pub fn muted() -> Style {
    base().fg(palette().muted)
}

pub fn warning() -> Style {
    base().fg(palette().warning).add_modifier(Modifier::BOLD)
}

pub fn selected() -> Style {
    title().add_modifier(Modifier::REVERSED)
}

pub fn panel<'a>() -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(muted())
        .title_style(title())
}
