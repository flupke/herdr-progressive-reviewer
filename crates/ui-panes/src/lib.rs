//! A reusable left selector and right content layout.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};
mod file_list;
mod tree;
pub use file_list::FileList;
pub use tree::{FileTree, FileTreeRow};

pub fn shorten(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthStr::width(character.to_string().as_str());
        if used + character_width >= width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push('…');
    result
}

pub fn pad_to_width(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let mut text = shorten(text, width);
    text.push_str(&" ".repeat(width.saturating_sub(UnicodeWidthStr::width(text.as_str()))));
    text
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitPane {
    pub left: Rect,
    pub right: Rect,
    pub divider: Option<Rect>,
}

impl SplitPane {
    pub fn new(area: Rect, preferred_left: u16, minimum_right: u16) -> Self {
        Self::with_minimums(area, preferred_left, 0, minimum_right)
    }

    pub fn with_minimums(
        area: Rect,
        preferred_left: u16,
        minimum_left: u16,
        minimum_right: u16,
    ) -> Self {
        let minimum_left = minimum_left.min(area.width.saturating_sub(1));
        let minimum_right = minimum_right.min(area.width.saturating_sub(minimum_left));
        let left_width = preferred_left.clamp(minimum_left, area.width - minimum_right);
        Self {
            left: Rect::new(area.x, area.y, left_width, area.height),
            right: Rect::new(
                area.x.saturating_add(left_width),
                area.y,
                area.width.saturating_sub(left_width),
                area.height,
            ),
            divider: None,
        }
    }

    pub fn with_divider(
        area: Rect,
        preferred_left: u16,
        minimum_left: u16,
        minimum_right: u16,
    ) -> Self {
        if area.width == 0 {
            return Self::with_minimums(area, preferred_left, minimum_left, minimum_right);
        }
        let content = Rect::new(area.x, area.y, area.width - 1, area.height);
        let mut panes = Self::with_minimums(content, preferred_left, minimum_left, minimum_right);
        panes.divider = Some(Rect::new(panes.left.right(), area.y, 1, area.height));
        panes.right.x = panes.right.x.saturating_add(1);
        panes
    }

    pub fn render_divider(self, buffer: &mut Buffer, style: Style) {
        if let Some(divider) = self.divider {
            for row in divider.top()..divider.bottom() {
                if let Some(cell) = buffer.cell_mut((divider.x, row)) {
                    cell.set_symbol("│").set_style(style);
                }
            }
        }
    }

    pub fn render(
        self,
        buffer: &mut Buffer,
        left: impl FnOnce(Rect, &mut Buffer),
        right: impl FnOnce(Rect, &mut Buffer),
    ) {
        left(self.left, buffer);
        right(self.right, buffer);
    }
}

/// A bordered selector with shared row geometry and caller-defined row content.
pub struct SelectionPane {
    area: Rect,
    first: usize,
}

impl SelectionPane {
    pub fn new(area: Rect, first: usize) -> Self {
        Self { area, first }
    }

    pub fn render<T>(
        &self,
        buffer: &mut Buffer,
        title: impl Into<Line<'static>>,
        border_style: Style,
        items: &[T],
        mut row: impl FnMut(usize, &T, u16) -> Line<'static>,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);
        let inner = block.inner(self.area);
        block.render(self.area, buffer);
        let lines = items
            .iter()
            .enumerate()
            .skip(self.first)
            .take(usize::from(inner.height))
            .map(|(index, item)| row(index, item, inner.width))
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
    }

    pub fn index_at(&self, column: u16, row: u16) -> Option<usize> {
        let inner = Rect::new(
            self.area.x.saturating_add(1),
            self.area.y.saturating_add(1),
            self.area.width.saturating_sub(2),
            self.area.height.saturating_sub(2),
        );
        inner
            .contains((column, row).into())
            .then(|| self.first + usize::from(row - inner.y))
    }

    pub fn index_for_component_row(first: usize, row: u16) -> usize {
        first + usize::from(row.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_right_pane_visible_when_the_preferred_left_is_too_wide() {
        let split = SplitPane::new(Rect::new(4, 2, 50, 8), 40, 24);
        assert_eq!(split.left, Rect::new(4, 2, 26, 8));
        assert_eq!(split.right, Rect::new(30, 2, 24, 8));
    }

    #[test]
    fn preserves_both_panes_at_narrow_widths() {
        let split = SplitPane::with_minimums(Rect::new(0, 0, 24, 8), 26, 8, 16);
        assert_eq!(split.left.width, 8);
        assert_eq!(split.right.width, 16);
    }
}
