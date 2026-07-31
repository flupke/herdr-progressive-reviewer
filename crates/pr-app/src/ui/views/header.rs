use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Widget};

use crate::review::ReviewStatus;
use crate::ui::ReviewApp;

pub(super) struct HeaderView<'a>(pub(super) &'a ReviewApp);

impl Widget for HeaderView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let reviewed = self
            .0
            .files
            .iter()
            .filter(|file| file.status == ReviewStatus::Reviewed)
            .count();
        Paragraph::new(format!(" {}", self.0.commit_title()))
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
        Paragraph::new(format!("{reviewed}/{} reviewed ", self.0.files.len()))
            .alignment(Alignment::Right)
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
    }
}
