use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::ui::ReviewApp;

const CONTROLS: &str =
    "Tab focus · j/k move · l expand · v select · c message · Enter insert · Space review · q quit";

pub(super) struct FooterView<'a>(pub(super) &'a ReviewApp);

impl Widget for FooterView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let status = match self.0.review_in_flight {
            Some(true) => "Marking reviewed…",
            Some(false) => "Removing review mark…",
            None => self.0.notice.as_deref().unwrap_or(""),
        };
        let lines = if area.height == 1 {
            vec![Line::from(vec![Span::raw(if status.is_empty() {
                CONTROLS
            } else {
                status
            })])]
        } else {
            vec![Line::raw(CONTROLS), Line::raw(status)]
        };
        Paragraph::new(lines)
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
    }
}
