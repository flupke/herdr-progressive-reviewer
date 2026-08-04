use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};

use crate::ReviewApp;
use crate::render::pane_block;

pub(super) struct CommitMessageView<'a>(pub(super) &'a ReviewApp);

impl CommitMessageView<'_> {
    pub(super) fn area(area: Rect) -> Rect {
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
