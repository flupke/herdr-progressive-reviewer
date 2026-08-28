use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use crate::ReviewApp;
use crate::popup::PopupView;

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
        let content = if self.0.description.is_empty() {
            "(no description set)"
        } else {
            &self.0.description
        };
        PopupView::new(self.0, "Commit message", content)
            .wrap()
            .render(popup, buffer);
    }
}
