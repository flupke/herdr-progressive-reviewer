//! The reader's place in one diff document: the cursor and the part of the
//! document on screen.
//!
//! [`Position`] is the only owner of the cursor row, its byte column and the
//! scroll offset. Its screen operations take the document's current
//! [`Layout`] and the screen height, and keep the cursor on screen: scrolling
//! pulls the cursor along, placing the screen for a moved cursor or a resized
//! screen brings it back into view, and a layout change keeps it on its
//! screen row.
//!
//! Some operations show other rows than the cursor's and leave it where it is:
//! [`Position::reveal_comment`] and [`Position::pin`] while a comment has the
//! focus and the cursor is not drawn, [`Position::reveal_evidence`] for
//! evidence that starts at the cursor, and [`Position::align_cursor_top`] for
//! a jump target taller than the screen.
//!
//! The layout depends on the cursor (an end-of-line cursor can wrap onto its
//! own row), so the cursor moves ([`Position::move_to`],
//! [`Position::set_column`], [`Position::restore`]) do not place the screen:
//! the caller lays the document out for the moved cursor, then places the
//! screen with it.

use std::ops::{Range, RangeInclusive};

/// Rows a scrolled-to evidence range keeps above it when they fit.
const EVIDENCE_CONTEXT_ROWS: usize = 3;

/// How one document fills the screen: its visual rows, after wrapping and
/// with comments and other inserted rows in place.
pub trait Layout {
    /// Number of visual rows in the whole document.
    fn row_count(&self) -> usize;

    /// Visual row that shows the cursor of `position`.
    fn cursor_row(&self, position: &Position) -> usize;

    /// First visual row of document row `row`, if it is shown.
    fn first_row_of(&self, row: usize) -> Option<usize>;

    /// Document row and byte column of the source row within `rows` nearest
    /// to the cursor of `position`, keeping its on-screen column where the row
    /// is long enough.
    fn nearest_cursor(&self, position: &Position, rows: Range<usize>) -> Option<(usize, usize)>;
}

/// Where the reader is in one document: the cursor's document row and byte
/// column, and the first visual row on screen.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Position {
    cursor: usize,
    column: usize,
    scroll: usize,
}

/// The cursor's place on screen, kept across a change to the layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenAnchor {
    visual_row: usize,
    scroll: usize,
}

impl Position {
    /// A position with these values, for example read back from saved state.
    /// [`Position::restore`] fits it to the current document.
    pub fn new(cursor: usize, column: usize, scroll: usize) -> Self {
        Self {
            cursor,
            column,
            scroll,
        }
    }

    /// Document row of the cursor.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Byte column of the cursor in its source line.
    pub fn column(&self) -> usize {
        self.column
    }

    /// First visual row on screen, as last placed.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// First visual row on screen in a document of `row_count` visual rows.
    pub fn top(&self, row_count: usize) -> usize {
        self.scroll.min(row_count.saturating_sub(1))
    }

    /// Move the cursor to `row` of a document with `rows` rows.
    pub fn move_to(&mut self, row: usize, rows: usize) {
        self.cursor = row.min(rows.saturating_sub(1));
    }

    /// Move the cursor to a byte column of its line.
    pub fn set_column(&mut self, column: usize) {
        self.column = column;
    }

    /// Take a saved position, keeping cursor and screen inside a document of
    /// `rows` rows.
    pub fn restore(&mut self, saved: Self, rows: usize) {
        let last = rows.saturating_sub(1);
        self.cursor = saved.cursor.min(last);
        self.scroll = saved.scroll.min(last);
        self.column = saved.column;
    }

    /// Take a saved position, keeping a full screen of `height` rows below its
    /// top.
    pub fn restore_filling_screen(&mut self, saved: Self, rows: usize, height: usize) {
        self.restore(saved, rows);
        self.scroll = self.scroll.min(rows.saturating_sub(height));
    }

    /// Scroll as little as possible to show the cursor, for example after it
    /// moved or the screen was resized.
    pub fn keep_cursor_visible(&mut self, layout: &impl Layout, height: usize) {
        let mut scroll = self.scroll.min(last_top(layout, height));
        let cursor = layout.cursor_row(self);
        if cursor < scroll {
            scroll = cursor;
        } else if cursor >= scroll.saturating_add(height) {
            scroll = cursor + 1 - height;
        }
        self.scroll = scroll;
    }

    /// Show the cursor in the middle of the screen, after a jump.
    pub fn center_cursor(&mut self, layout: &impl Layout, height: usize) {
        self.scroll = layout
            .cursor_row(self)
            .saturating_sub(height / 2)
            .min(last_top(layout, height));
    }

    /// Show the cursor's row at the top of the screen.
    pub fn align_cursor_top(&mut self, layout: &impl Layout) {
        self.scroll = layout
            .first_row_of(self.cursor)
            .unwrap_or_else(|| layout.cursor_row(self));
    }

    /// Scroll by `delta` visual rows and bring the cursor along when it left
    /// the screen. Returns whether the cursor moved.
    pub fn scroll_by(&mut self, delta: isize, layout: &impl Layout, height: usize) -> bool {
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(last_top(layout, height));
        self.contain_cursor(layout, height)
    }

    /// Move an off-screen cursor to the nearest source row on screen. Returns
    /// whether the cursor moved.
    pub fn contain_cursor(&mut self, layout: &impl Layout, height: usize) -> bool {
        let row_count = layout.row_count();
        let cursor = layout.cursor_row(self);
        let top = self.top(row_count);
        let visible = top..top.saturating_add(height).min(row_count);
        if visible.contains(&cursor) {
            return false;
        }
        let Some((row, column)) = layout.nearest_cursor(self, visible) else {
            return false;
        };
        self.cursor = row;
        self.column = column;
        true
    }

    /// Remember where the cursor is on screen before the layout changes, for
    /// example when context is folded or unfolded.
    pub fn screen_anchor(&self, layout: &impl Layout) -> ScreenAnchor {
        ScreenAnchor {
            visual_row: layout.cursor_row(self),
            scroll: self.scroll,
        }
    }

    /// Keep the cursor on the screen row it had at `anchor` in the new layout.
    pub fn return_to(&mut self, anchor: ScreenAnchor, layout: &impl Layout) {
        self.scroll = anchor
            .scroll
            .saturating_add(layout.cursor_row(self))
            .saturating_sub(anchor.visual_row);
    }

    /// Keep visual row `row` at `screen_row` of the screen, for example when
    /// posted comments are placed above what the reader was looking at.
    pub fn pin(&mut self, row: usize, screen_row: usize, layout: &impl Layout, height: usize) {
        self.scroll = row.saturating_sub(screen_row).min(last_top(layout, height));
    }

    /// Reveal the comment on visual `rows`. An edited comment is shown whole
    /// when it fits, otherwise its end is. A read comment is shown from the
    /// row above it when any of it is off screen.
    pub fn reveal_comment(
        &mut self,
        rows: RangeInclusive<usize>,
        editing: bool,
        layout: &impl Layout,
        height: usize,
    ) {
        let (start, end) = rows.into_inner();
        if editing {
            let bottom = end.saturating_add(1).saturating_sub(height);
            self.scroll = if bottom <= start {
                self.scroll.clamp(bottom, start)
            } else {
                bottom
            };
        } else if start < self.scroll || end >= self.scroll.saturating_add(height) {
            self.scroll = start.saturating_sub(1).min(last_top(layout, height));
        }
    }

    /// Reveal evidence on visual `rows` of `row_count`, with a little context
    /// above it when it fits.
    pub fn reveal_evidence(&mut self, rows: Range<usize>, row_count: usize, height: usize) {
        let before = height.saturating_sub(rows.len()).min(EVIDENCE_CONTEXT_ROWS);
        self.scroll = rows
            .start
            .saturating_sub(before)
            .min(row_count.saturating_sub(height));
    }
}

/// Top row of the last full screen.
fn last_top(layout: &impl Layout, height: usize) -> usize {
    layout.row_count().saturating_sub(height)
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
