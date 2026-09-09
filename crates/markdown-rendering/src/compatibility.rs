//! Keep ratatui-markdown's 0.29 types at the rendering boundary until it supports 0.30.

use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui_legacy::{layout as legacy_layout, style as legacy_style, text as legacy_text};
use ratatui_markdown::theme::ThemeConfig;
use ui_theme::Palette;

pub(super) struct MarkdownStyles(Palette);

impl MarkdownStyles {
    pub(super) fn new(palette: Palette) -> Self {
        Self(palette)
    }

    pub(super) fn theme(&self) -> ThemeConfig {
        ThemeConfig::default()
            .with_text_color(Self::legacy_color(self.0.text))
            .with_muted_text_color(Self::legacy_color(self.0.dim))
            .with_primary_color(Self::legacy_color(self.0.focus))
            .with_secondary_color(Self::legacy_color(self.0.focus))
            .with_info_color(Self::legacy_color(self.0.focus))
            .with_accent_yellow(Self::legacy_color(self.0.warning))
    }

    pub(super) fn legacy_color(color: Color) -> legacy_style::Color {
        color
            .to_string()
            .parse()
            .expect("shared Ratatui color syntax")
    }

    fn color(color: legacy_style::Color) -> Color {
        color
            .to_string()
            .parse()
            .expect("shared Ratatui color syntax")
    }

    fn style(style: legacy_style::Style) -> Style {
        Style {
            fg: style.fg.map(Self::color),
            bg: style.bg.map(Self::color),
            underline_color: style.underline_color.map(Self::color),
            add_modifier: Modifier::from_bits_retain(style.add_modifier.bits()),
            sub_modifier: Modifier::from_bits_retain(style.sub_modifier.bits()),
        }
    }

    pub(super) fn line(line: legacy_text::Line<'_>) -> Line<'_> {
        Line {
            spans: line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content, Self::style(span.style)))
                .collect(),
            style: Self::style(line.style),
            alignment: line.alignment.map(|alignment| match alignment {
                legacy_layout::Alignment::Left => Alignment::Left,
                legacy_layout::Alignment::Center => Alignment::Center,
                legacy_layout::Alignment::Right => Alignment::Right,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_styles_preserve_colors_modifiers_and_alignment() {
        let style = legacy_style::Style::default()
            .fg(legacy_style::Color::Rgb(12, 34, 56))
            .bg(legacy_style::Color::Indexed(123))
            .underline_color(legacy_style::Color::LightCyan)
            .add_modifier(legacy_style::Modifier::BOLD | legacy_style::Modifier::ITALIC)
            .remove_modifier(legacy_style::Modifier::DIM);
        let line = legacy_text::Line::from(legacy_text::Span::styled("fn main()", style))
            .style(legacy_style::Style::default().fg(legacy_style::Color::Reset))
            .centered();
        let converted = MarkdownStyles::line(line);
        assert_eq!(converted.alignment, Some(Alignment::Center));
        assert_eq!(converted.style.fg, Some(Color::Reset));
        assert_eq!(converted.spans[0].content, "fn main()");
        assert_eq!(
            converted.spans[0].style,
            Style::default()
                .fg(Color::Rgb(12, 34, 56))
                .bg(Color::Indexed(123))
                .underline_color(Color::LightCyan)
                .add_modifier(Modifier::BOLD | Modifier::ITALIC)
                .remove_modifier(Modifier::DIM)
        );
    }
}
