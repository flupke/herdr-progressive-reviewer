//! Application pane layout and focus state.

use ratatui::layout::Rect;

const NARROW_WIDTH: u16 = 72;
const MINIMUM_PANE_WIDTH: u16 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Focus {
    Files,
    Diff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PaneLayout {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) footer_height: u16,
    pub(crate) file_width: u16,
}

impl PaneLayout {
    pub(crate) fn new(width: u16, height: u16, file_width: Option<u16>) -> Self {
        let file_width = if width >= NARROW_WIDTH {
            file_width
                .unwrap_or(width * 30 / 100)
                .clamp(MINIMUM_PANE_WIDTH, width - MINIMUM_PANE_WIDTH)
        } else {
            width
        };
        Self {
            width,
            height,
            footer_height: 1,
            file_width,
        }
    }

    pub(crate) fn is_wide(self) -> bool {
        self.width >= NARROW_WIDTH
    }

    pub(crate) fn body_height(self) -> u16 {
        self.height.saturating_sub(1 + self.footer_height)
    }

    pub(crate) fn contains_body(self, column: u16, row: u16) -> bool {
        column < self.width && row > 0 && row < self.height.saturating_sub(self.footer_height)
    }

    pub(crate) fn is_separator(self, column: u16, row: u16) -> bool {
        self.is_wide() && self.contains_body(column, row) && column.abs_diff(self.file_width) <= 1
    }

    pub(crate) fn page_rows(self) -> usize {
        usize::from(self.body_height().saturating_sub(2).max(1))
    }

    pub(crate) fn files_content_area(self, focus: Focus) -> Option<Rect> {
        if !self.is_wide() && focus != Focus::Files {
            return None;
        }
        let pane_width = if self.is_wide() {
            self.file_width
        } else {
            self.width
        };
        Some(Rect::new(
            1,
            2,
            pane_width.saturating_sub(2),
            self.body_height().saturating_sub(2),
        ))
    }
}
