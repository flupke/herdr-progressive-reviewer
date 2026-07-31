use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Widget};

use crate::ui::ReviewApp;

const CONTROLS: &str =
    "Tab focus · j/k move · l expand · v select · c message · Enter insert · Space review · q quit";

pub(super) struct FooterView<'a>(pub(super) &'a ReviewApp);

impl Widget for FooterView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Paragraph::new(CONTROLS)
            .style(Style::default().fg(self.0.palette.text))
            .render(area, buffer);
    }
}
