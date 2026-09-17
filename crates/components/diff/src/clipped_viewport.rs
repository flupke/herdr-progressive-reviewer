//! One projection for embedded rendering, pointer input and reply visibility.
use ratatui::{buffer::Buffer, layout::Rect};

#[derive(Clone, Copy)]
pub struct ClippedViewport {
    visible: Rect,
    skipped: u16,
    height: u16,
    pinned_header: bool,
}

impl ClippedViewport {
    pub fn new(visible: Rect, skipped: u16, height: u16) -> Self {
        Self {
            visible,
            skipped,
            height,
            pinned_header: false,
        }
    }

    #[must_use]
    pub fn with_pinned_header(mut self) -> Self {
        self.pinned_header = true;
        self
    }

    pub fn area(self) -> Rect {
        Rect::new(0, 0, self.visible.width, self.height)
    }

    fn source_row(self, visible_row: u16) -> u16 {
        if self.pinned_header && self.skipped > 0 && visible_row == 0 {
            0
        } else {
            visible_row
                .saturating_add(self.skipped)
                .min(self.height.saturating_sub(1))
        }
    }

    pub fn local_position(self, column: u16, row: u16) -> (u16, u16) {
        (
            column.saturating_sub(self.visible.x),
            self.source_row(row.saturating_sub(self.visible.y)),
        )
    }

    pub fn draw(self, source: &Buffer, destination: &mut Buffer) {
        for row in 0..self.visible.height {
            for column in 0..self.visible.width {
                if let Some(cell) = source.cell((column, self.source_row(row))) {
                    destination[(self.visible.x + column, self.visible.y + row)] = cell.clone();
                }
            }
        }
    }

    pub(super) fn project_row(self, area: Rect) -> Option<Rect> {
        let relative = area.y.checked_sub(self.skipped)?;
        if relative >= self.visible.height || self.source_row(relative) != area.y {
            return None;
        }
        Some(Rect::new(
            self.visible.x + area.x,
            self.visible.y + relative,
            area.width,
            area.height,
        ))
    }
}
