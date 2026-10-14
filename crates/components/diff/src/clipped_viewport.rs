//! One projection for embedded rendering, pointer input and reply visibility.
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ui_text_selection::TextRegion;

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

    /// `region`, drawn in the window, where this viewport shows its rows.
    pub(super) fn place_text(self, region: &TextRegion) -> TextRegion {
        let area = region.area();
        let mut shown = (area.top()..area.bottom())
            .filter_map(|y| self.project_row(Rect::new(area.x, y, area.width, 1)));
        let placed = shown.next().map_or_else(Rect::default, |first| {
            let last = shown.next_back().unwrap_or(first);
            first.union(last).intersection(self.visible)
        });
        region.placed(placed, |position| {
            self.project_row(Rect::new(position.x, position.y, 1, 1))
                .map(|placed| Position::new(placed.x, placed.y))
        })
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

#[cfg(test)]
#[path = "clipped_viewport.tests.rs"]
mod tests;
