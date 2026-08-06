use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::ReviewApp;
use review_state::ReviewStatus;

pub(super) struct HeaderView<'a>(pub(super) &'a ReviewApp);

impl Widget for HeaderView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let reviewed = self
            .0
            .files
            .iter()
            .filter(|file| file.status == ReviewStatus::Reviewed)
            .count();
        let added = self
            .0
            .files
            .iter()
            .map(|file| file.lines_added)
            .sum::<u64>();
        let removed = self
            .0
            .files
            .iter()
            .map(|file| file.lines_removed)
            .sum::<u64>();
        Paragraph::new(format!(" {}", self.0.commit_title()))
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("+{added}"),
                Style::default().fg(self.0.palette.insertion),
            ),
            Span::raw(" "),
            Span::styled(
                format!("-{removed}"),
                Style::default().fg(self.0.palette.deletion),
            ),
            Span::raw(format!(" - {reviewed}/{} reviewed ", self.0.files.len())),
        ]))
        .alignment(Alignment::Right)
        .style(Style::default().fg(self.0.palette.text))
        .render(area, buffer);
    }
}

#[cfg(test)]
#[path = "header.tests.rs"]
mod tests;
