use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ui_theme::Palette;

use crate::popup::{centered_area, render_popup};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct CommitMessageOverlay {
    description: String,
}

impl CommitMessageOverlay {
    pub(super) fn replace_description(&mut self, description: &str) {
        self.description.clear();
        self.description.push_str(description);
    }

    pub(super) fn area(area: Rect) -> Rect {
        centered_area(
            area,
            area.width.saturating_mul(4) / 5,
            area.height.saturating_mul(4) / 5,
        )
    }

    pub(super) fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        let content = if self.description.is_empty() {
            "(no description set)"
        } else {
            &self.description
        };
        render_popup(
            Self::area(area),
            buffer,
            "Commit message",
            Text::raw(content),
            true,
            0,
            palette,
        );
    }
}
