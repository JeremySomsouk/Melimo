use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType},
};

const BACKGROUND: Color = Color::Rgb(13, 15, 22);
const FOREGROUND: Color = Color::Rgb(246, 239, 226);
const VIOLET: Color = Color::Rgb(170, 116, 255);
const MUTED: Color = Color::Rgb(165, 166, 188);
const BORDER: Color = Color::Rgb(65, 69, 87);
const TRACK: Color = Color::Rgb(35, 38, 52);

pub struct Theme {
    monochrome: bool,
    light: bool,
}

impl Theme {
    pub fn from_env() -> Self {
        let mode = std::env::var("MELIMO_THEME").unwrap_or_default();
        Self {
            light: mode == "light",
            monochrome: mode == "mono"
                || std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
                || std::env::var("TERM").is_ok_and(|value| value == "dumb"),
        }
    }

    fn color(&self, dark: Color, light: Color) -> Color {
        if self.light { light } else { dark }
    }

    pub fn base(&self) -> Style {
        if self.monochrome {
            Style::default()
        } else {
            Style::default()
                .fg(self.color(FOREGROUND, Color::Rgb(40, 33, 52)))
                .bg(self.color(BACKGROUND, Color::Rgb(250, 248, 253)))
        }
    }

    pub fn title(&self) -> Style {
        self.base().add_modifier(Modifier::BOLD)
    }

    pub fn accent(&self) -> Style {
        if self.monochrome {
            self.title()
        } else {
            self.title()
                .fg(self.color(VIOLET, Color::Rgb(105, 55, 160)))
        }
    }

    pub fn selected(&self) -> Style {
        if self.monochrome {
            self.title().add_modifier(Modifier::REVERSED)
        } else {
            self.title()
                .fg(self.color(BACKGROUND, Color::Rgb(250, 248, 253)))
                .bg(self.color(VIOLET, Color::Rgb(105, 55, 160)))
        }
    }

    pub fn secondary(&self) -> Style {
        if self.monochrome {
            self.base().add_modifier(Modifier::DIM)
        } else {
            self.base().fg(self.color(MUTED, Color::Rgb(103, 94, 116)))
        }
    }

    pub fn progress(&self) -> Style {
        if self.monochrome {
            self.base().fg(Color::White).bg(Color::Black)
        } else {
            self.base()
                .fg(self.color(VIOLET, Color::Rgb(105, 55, 160)))
                .bg(self.color(TRACK, Color::Rgb(228, 222, 237)))
        }
    }

    pub fn panel<'a>(&self, title: impl Into<Line<'a>>) -> Block<'a> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .style(self.base())
            .border_style(if self.monochrome {
                self.base()
            } else {
                self.base()
                    .fg(self.color(BORDER, Color::Rgb(150, 140, 164)))
            })
            .title_style(self.accent())
            .title(title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_secondary_and_accent_text_have_readable_contrast() {
        fn luminance(color: Color) -> f64 {
            let Color::Rgb(r, g, b) = color else {
                panic!("expected RGB palette")
            };
            [r, g, b]
                .into_iter()
                .zip([0.2126, 0.7152, 0.0722])
                .map(|(channel, weight)| {
                    let value = f64::from(channel) / 255.0;
                    weight
                        * if value <= 0.04045 {
                            value / 12.92
                        } else {
                            ((value + 0.055) / 1.055).powf(2.4)
                        }
                })
                .sum()
        }
        for foreground in [FOREGROUND, MUTED, VIOLET] {
            let ratio = (luminance(foreground) + 0.05) / (luminance(BACKGROUND) + 0.05);
            assert!(ratio >= 4.5, "insufficient contrast: {ratio}");
        }
    }

    #[test]
    fn monochrome_preserves_terminal_colors_and_selection_cue() {
        let theme = Theme {
            monochrome: true,
            light: false,
        };
        for style in [
            theme.base(),
            theme.title(),
            theme.accent(),
            theme.secondary(),
        ] {
            assert_eq!(style.fg, None);
            assert_eq!(style.bg, None);
        }
        assert!(theme.selected().add_modifier.contains(Modifier::REVERSED));
    }
}
