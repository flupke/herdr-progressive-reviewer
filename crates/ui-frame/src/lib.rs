//! The rounded frames around the reviewer's panes, popups, cards and notices.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType};
use ui_theme::Palette;

/// What a frame surrounds, which decides how much it draws attention.
///
/// Only the frame that has the user's attention takes the accent color;
/// the others recede, so focus reads from the borders alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    /// A region that can hold focus, such as a pane or an input field.
    Pane { focused: bool },
    /// A window over the layout, which always has the user's attention.
    Popup,
    /// One card in a list.
    Card { selected: bool },
    /// A notice framed and titled in its own color.
    Notice(Color),
}

impl Frame {
    /// A bordered block with `title`, padded by a space, on its top border.
    pub fn block<'a>(self, palette: Palette, title: impl Into<Line<'a>>) -> Block<'a> {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.border_style(palette));
        let title = title.into();
        if title.width() == 0 {
            return block;
        }
        let mut spans = Vec::with_capacity(title.spans.len() + 2);
        spans.push(Span::raw(" "));
        spans.extend(title.spans);
        spans.push(Span::raw(" "));
        let mut padded = Line::from(spans).style(self.title_style(palette).patch(title.style));
        padded.alignment = title.alignment;
        block.title(padded)
    }

    fn has_attention(self) -> bool {
        match self {
            Self::Pane { focused } => focused,
            Self::Card { selected } => selected,
            Self::Popup | Self::Notice(_) => true,
        }
    }

    /// The style of the border's lines, for frames drawn line by line.
    pub fn border_style(self, palette: Palette) -> Style {
        Style::default().fg(match self {
            Self::Notice(color) => color,
            _ if self.has_attention() => palette.focus,
            _ => palette.border,
        })
    }

    fn title_style(self, palette: Palette) -> Style {
        match self {
            Self::Notice(color) => Style::default().fg(color).add_modifier(Modifier::BOLD),
            _ if self.has_attention() => Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
            _ => Style::default().fg(palette.dim),
        }
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
