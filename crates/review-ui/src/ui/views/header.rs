use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::ui::ReviewApp;
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
mod tests {
    use super::*;
    use crate::ui::ReviewFile;

    #[test]
    fn totals_file_stats_in_the_header() {
        let mut app = ReviewApp::default();
        let mut reviewed = ReviewFile::new("first.rs", ReviewStatus::Reviewed);
        reviewed.lines_added = 5;
        reviewed.lines_removed = 2;
        let mut unreviewed = ReviewFile::new("second.rs", ReviewStatus::Unreviewed);
        unreviewed.lines_added = 3;
        unreviewed.lines_removed = 4;
        app.files = vec![reviewed, unreviewed];
        let area = Rect::new(0, 0, 80, 1);
        let mut buffer = Buffer::empty(area);

        HeaderView(&app).render(area, &mut buffer);

        let text = (0..area.width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>();
        assert!(text.contains("+8 -6 - 1/2 reviewed"));
        assert_eq!(
            buffer[(u16::try_from(text.find("+8").unwrap()).unwrap(), 0)].fg,
            app.palette.insertion
        );
        assert_eq!(
            buffer[(u16::try_from(text.find("-6").unwrap()).unwrap(), 0)].fg,
            app.palette.deletion
        );
    }
}
