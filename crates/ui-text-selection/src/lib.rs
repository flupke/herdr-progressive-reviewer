//! Text the reviewer selects on screen, by dragging, to copy it.
//!
//! A text selection copies the text a pane shows rather than the screen: each pane lays its
//! content out in regions of rows that leave out gutters and borders, and says where a
//! line it wrapped continues on the next row.

mod screen;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

/// One grapheme of content and the cells it takes on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextCell {
    text: String,
    width: u16,
}

impl TextCell {
    /// `text` shown over `width` cells: a tab can show as several spaces.
    pub fn new(text: impl Into<String>, width: u16) -> Self {
        Self {
            text: text.into(),
            width: width.max(1),
        }
    }

    fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// How a row's text meets the next row's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowEnd {
    /// The line ends with the row.
    Line,
    /// The pane wrapped the line onto the next row, dropping a space there when `space`.
    Wrap { space: bool },
}

/// The content one screen row shows, from its `position`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRow {
    position: Position,
    cells: Vec<TextCell>,
    end: RowEnd,
}

impl TextRow {
    pub fn new(position: Position, cells: Vec<TextCell>, end: RowEnd) -> Self {
        Self {
            position,
            cells,
            end,
        }
    }

    /// Each cell with the screen column it starts at.
    fn columns(&self) -> impl Iterator<Item = (u16, &TextCell)> {
        self.cells.iter().scan(self.position.x, |column, cell| {
            let start = *column;
            *column = column.saturating_add(cell.width);
            Some((start, cell))
        })
    }
}

/// The press that starts a text selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextPress {
    /// A plain press, which starts one where a drag means nothing else.
    #[default]
    Plain,
    /// A press with Alt held, which starts one anywhere.
    Alt,
}

/// A part of the screen that a text selection stays inside, such as a pane's content.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextRegion {
    area: Rect,
    rows: Vec<TextRow>,
    /// The press a text selection needs to start here.
    needs: TextPress,
}

impl TextRegion {
    pub fn new(area: Rect) -> Self {
        Self {
            area,
            rows: Vec::new(),
            needs: TextPress::Plain,
        }
    }

    /// The part of the screen the region covers.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// This region, needing Alt held to start a text selection: for where a plain drag means
    /// something else, such as commenting on lines.
    #[must_use]
    pub fn needing_alt(self) -> Self {
        Self {
            needs: TextPress::Alt,
            ..self
        }
    }

    /// Add a row; rows outside the region's area are dropped.
    pub fn push(&mut self, row: TextRow) {
        if self.area.contains(row.position) {
            let index = self
                .rows
                .partition_point(|other| other.position.y <= row.position.y);
            self.rows.insert(index, row);
        }
    }

    /// The region drawn off screen, then copied to `area` where `place` puts each row's
    /// start, or nowhere for a row the copy leaves out.
    #[must_use]
    pub fn placed(&self, area: Rect, place: impl Fn(Position) -> Option<Position>) -> Self {
        let mut placed = Self {
            needs: self.needs,
            ..Self::new(area)
        };
        for row in &self.rows {
            if let Some(position) = place(row.position) {
                placed.push(TextRow {
                    position,
                    ..row.clone()
                });
            }
        }
        placed
    }
}

/// The text regions of the last frame drawn, later ones on top.
#[derive(Clone, Debug, Default)]
pub struct TextLayer {
    regions: Vec<TextRegion>,
}

impl TextLayer {
    pub fn push(&mut self, region: TextRegion) {
        self.regions.push(region);
    }

    /// Start a text selection at `position`, in the topmost region that holds it, unless
    /// that region needs Alt held and `press` is plain.
    pub fn select(&self, position: Position, press: TextPress) -> Option<TextSelection> {
        let region = self
            .regions
            .iter()
            .rev()
            .find(|region| region.area.contains(position))?;
        if region.needs == TextPress::Alt && press == TextPress::Plain {
            return None;
        }
        Some(TextSelection {
            region: region.clone(),
            anchor: position,
            cursor: position,
        })
    }
}

/// The text between an anchor and a cursor, in reading order, inside one region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSelection {
    region: TextRegion,
    anchor: Position,
    cursor: Position,
}

impl TextSelection {
    /// Move the cursor to `position`, kept inside the region.
    pub fn extend(&mut self, position: Position) {
        let area = self.region.area;
        self.cursor = Position::new(
            position
                .x
                .clamp(area.x, area.right().saturating_sub(1).max(area.x)),
            position
                .y
                .clamp(area.y, area.bottom().saturating_sub(1).max(area.y)),
        );
    }

    /// The selected text, or none when the cursor never left the anchor.
    pub fn text(&self) -> Option<String> {
        if self.anchor == self.cursor {
            return None;
        }
        let mut text = String::new();
        let mut previous: Option<&TextRow> = None;
        for (row, cells) in self.selected_rows() {
            if let Some(previous) = previous {
                text.push_str(match previous.end {
                    RowEnd::Wrap { space } if previous.position.y + 1 == row.position.y => {
                        if space { " " } else { "" }
                    }
                    RowEnd::Wrap { .. } | RowEnd::Line => "\n",
                });
            }
            for (_, cell) in cells {
                text.push_str(&cell.text);
            }
            previous = Some(row);
        }
        Some(text)
    }

    /// Paint the selected cells with `style`, or none when the cursor never left the anchor.
    pub fn highlight(&self, buffer: &mut Buffer, style: Style) {
        if self.anchor == self.cursor {
            return;
        }
        for (row, cells) in self.selected_rows() {
            for (column, cell) in cells {
                for offset in 0..cell.width {
                    if let Some(painted) =
                        buffer.cell_mut((column.saturating_add(offset), row.position.y))
                    {
                        painted.set_style(style);
                    }
                }
            }
        }
    }

    fn ordered(&self) -> (Position, Position) {
        let key = |position: Position| (position.y, position.x);
        if key(self.anchor) <= key(self.cursor) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    fn selected_rows(&self) -> impl Iterator<Item = (&TextRow, Vec<(u16, &TextCell)>)> {
        let (start, end) = self.ordered();
        self.region
            .rows
            .iter()
            .filter(move |row| (start.y..=end.y).contains(&row.position.y))
            .map(move |row| {
                let y = row.position.y;
                let cells = row
                    .columns()
                    .filter(|(column, cell)| {
                        (y > start.y || column.saturating_add(cell.width) > start.x)
                            && (y < end.y || *column <= end.x)
                    })
                    .collect();
                (row, cells)
            })
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
