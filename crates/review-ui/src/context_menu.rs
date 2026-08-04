use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::app::{ContextMenu, ReviewApp};

pub(super) struct ContextMenuView<'a>(pub(super) &'a ReviewApp, pub(super) &'a ContextMenu);

impl Widget for ContextMenuView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let popup = self.1.area(area);
        Clear.render(popup, buffer);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.1.enabled {
                self.0.palette.focus
            } else {
                self.0.palette.dim
            }));
        let inner = block.inner(popup);
        block.render(popup, buffer);
        let labels = ["Documentation", "Go to definition", "Find references"];
        let lines = labels.into_iter().enumerate().map(|(index, label)| {
            let style = Style::default()
                .fg(if self.1.enabled {
                    self.0.palette.text
                } else {
                    self.0.palette.dim
                })
                .bg(if index == self.1.selected {
                    self.0.palette.cursor
                } else {
                    Color::Reset
                });
            Line::styled(label, style)
        });
        Paragraph::new(lines.collect::<Vec<_>>()).render(inner, buffer);
    }
}
