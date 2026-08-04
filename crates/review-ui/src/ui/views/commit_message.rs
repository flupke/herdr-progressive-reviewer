use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};

use super::pane_block;
use crate::ui::ReviewApp;

pub(in crate::ui) struct CommitMessageView<'a>(pub(super) &'a ReviewApp);

impl CommitMessageView<'_> {
    pub(in crate::ui) fn area(area: Rect) -> Rect {
        let width = area.width.saturating_mul(4) / 5;
        let height = area.height.saturating_mul(4) / 5;
        Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        )
    }
}

impl Widget for CommitMessageView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let popup = Self::area(area);
        Clear.render(popup, buffer);
        Paragraph::new(if self.0.description.is_empty() {
            "(no description set)"
        } else {
            &self.0.description
        })
        .block(pane_block(self.0, "Commit message", true))
        .wrap(Wrap { trim: false })
        .render(popup, buffer);
    }
}
