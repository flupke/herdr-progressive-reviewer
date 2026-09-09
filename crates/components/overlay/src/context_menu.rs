use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use review_lsp::Operation;
use ui_events::LspQueryContext;
use ui_theme::Palette;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SourceContextMenu {
    pub(super) column: u16,
    pub(super) row: u16,
    pub(super) selected: usize,
    pub(super) query: Option<LspQueryContext>,
}

impl SourceContextMenu {
    pub(super) fn area(&self, bounds: Rect) -> Rect {
        let width = 22.min(bounds.width);
        let height = 5.min(bounds.height);
        Rect::new(
            self.column.min(bounds.right().saturating_sub(width)),
            self.row.min(bounds.bottom().saturating_sub(height)),
            width,
            height,
        )
    }

    pub(super) fn move_down(&mut self) {
        self.selected = (self.selected + 1).min(2);
    }

    pub(super) fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub(super) fn selected_query(self) -> Option<(Operation, LspQueryContext)> {
        Some((Operation::from_repr(self.selected)?, self.query?))
    }

    pub(super) fn query_at_row(self, row: u16, area: Rect) -> Option<(Operation, LspQueryContext)> {
        if row >= area.bottom().saturating_sub(1) {
            return None;
        }
        let item = row.checked_sub(area.y.saturating_add(1)).map(usize::from)?;
        Some((Operation::from_repr(item)?, self.query?))
    }

    pub(super) fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        let popup = self.area(area);
        Clear.render(popup, buffer);
        let enabled = self.query.is_some();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if enabled { palette.focus } else { palette.dim }));
        let inner = block.inner(popup);
        block.render(popup, buffer);
        let labels = ["Documentation", "Go to definition", "Find references"];
        let lines = labels.into_iter().enumerate().map(|(index, label)| {
            let style = Style::default()
                .fg(if enabled { palette.text } else { palette.dim })
                .bg(if index == self.selected {
                    palette.cursor
                } else {
                    Color::Reset
                });
            Line::styled(label, style)
        });
        Paragraph::new(lines.collect::<Vec<_>>()).render(inner, buffer);
    }
}
