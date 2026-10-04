//! Shared visual controls: actions are buttons, destinations are links.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use ui_theme::Palette;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonTone {
    Primary,
    Secondary,
    /// An action that discards work.
    Danger,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionButton {
    label: String,
    tone: ButtonTone,
}

impl ActionButton {
    pub fn new(label: impl Into<String>, tone: ButtonTone) -> Self {
        Self {
            label: label.into(),
            tone,
        }
    }

    pub fn text(&self) -> String {
        format!(" {} ", self.label)
    }

    pub fn style(&self, palette: Palette) -> Style {
        let (foreground, background) = match self.tone {
            ButtonTone::Primary => (palette.background, palette.insertion),
            ButtonTone::Secondary => (palette.text, palette.selection),
            ButtonTone::Danger => (palette.background, palette.deletion),
        };
        Style::default()
            .fg(foreground)
            .bg(background)
            .add_modifier(Modifier::BOLD)
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        render_label(self.text(), self.style(palette), area, buffer);
    }

    /// Render an already sized button label inside a composite line.
    pub fn span_with_text(&self, text: impl Into<String>, palette: Palette) -> Span<'static> {
        Span::styled(text.into(), self.style(palette))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationLink {
    label: String,
}

impl NavigationLink {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }

    pub fn text(&self) -> String {
        format!("[{}]", self.label)
    }

    pub fn style(palette: Palette) -> Style {
        Style::default().fg(palette.focus)
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        render_label(self.text(), Self::style(palette), area, buffer);
    }
}

/// A key and what it does, as the footer shows it: the key stands out, its label is dim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyHint {
    key: String,
    label: String,
}

impl KeyHint {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }

    /// Width in terminal cells.
    pub fn width(&self) -> u16 {
        label_width(&self.key).saturating_add(1 + label_width(&self.label))
    }

    pub fn spans(&self, palette: Palette) -> [Span<'static>; 2] {
        [
            Span::styled(
                self.key.clone(),
                Style::default()
                    .fg(palette.focus)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", self.label), Style::default().fg(palette.dim)),
        ]
    }
}

fn render_label(text: String, style: Style, area: Rect, buffer: &mut Buffer) {
    let width = label_width(&text).min(area.width);
    Paragraph::new(text)
        .style(style)
        .render(Rect::new(area.x, area.y, width, area.height.min(1)), buffer);
}

/// Width of a control label in terminal cells, as used when rendering it.
pub fn label_width(text: &str) -> u16 {
    u16::try_from(Line::raw(text).width()).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn action_background_stops_at_the_label_even_in_a_wide_row() {
        let palette = ui_theme::Theme::default().palette;
        let area = Rect::new(0, 0, 30, 1);
        let mut buffer = Buffer::empty(area);
        ActionButton::new("Start", ButtonTone::Primary).render(area, &mut buffer, palette);

        assert_eq!(buffer.cell((6, 0)).unwrap().bg, palette.insertion);
        assert_eq!(buffer.cell((7, 0)).unwrap().bg, Color::Reset);
    }
}
