//! Word-wrapped display coordinates kept separate from Edtui's editable lines.

use std::ops::Range;

use edtui::{EditorMode, EditorState, Index2, actions::Execute};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ui_theme::Palette;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub(super) struct EditorViewport {
    top: usize,
    source_top: usize,
    width: u16,
    height: u16,
    goal: Option<(Index2, usize)>,
}

struct EditorRow {
    line: Line<'static>,
    source_row: usize,
    cursor: Option<usize>,
    positions: Vec<(usize, usize)>,
}

struct EditorGlyph {
    span: Span<'static>,
    width: usize,
    whitespace: bool,
    cursor: bool,
    source_column: usize,
}

impl EditorViewport {
    pub(super) fn reset_motion(&mut self) {
        self.goal = None;
    }

    pub(super) fn move_cursor(
        &mut self,
        state: &mut EditorState,
        from: Index2,
        motion: super::motion::VisualMotion,
    ) {
        if self.width == 0 {
            return;
        }
        state.cursor = from;
        state.set_viewport_offset(0, self.source_top);
        let rows = EditorRow::document(state, self.width, ui_theme::Theme::default().palette);
        let current = rows
            .iter()
            .position(|row| row.cursor.is_some())
            .unwrap_or_default();
        let column = self.goal.filter(|(cursor, _)| *cursor == from).map_or_else(
            || rows[current].cursor.unwrap_or_default(),
            |(_, column)| column,
        );
        let distance = motion.step.distance(self.height);
        let delta = motion.direction * isize::try_from(distance).unwrap_or(isize::MAX);
        let target = current.saturating_add_signed(delta).min(rows.len() - 1);
        let row = &rows[target];
        let source_column = row
            .positions
            .iter()
            .rev()
            .find(|(x, _)| *x <= column)
            .or_else(|| row.positions.first())
            .map_or(0, |(_, col)| *col);
        state.cursor = Index2::new(row.source_row, source_column);
        self.goal = Some((state.cursor, column));
        if motion.step != super::motion::VisualStep::Row {
            self.top = self
                .top
                .saturating_add_signed(delta)
                .min(rows.len().saturating_sub(usize::from(self.height)));
        }
        // Update native visual and linewise selections at the final source position.
        edtui::actions::MoveDown(0).execute(state);
    }

    pub(super) fn render(
        &mut self,
        state: &mut EditorState,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
    ) {
        if self.width != area.width {
            self.reset_motion();
        }
        self.width = area.width;
        self.height = area.height;
        let rows = EditorRow::document(state, area.width, palette);
        let cursor_row = rows
            .iter()
            .position(|row| row.cursor.is_some())
            .unwrap_or(0);
        let requested_top = state.viewport_offset().1;
        if requested_top != self.source_top {
            self.top = rows
                .iter()
                .position(|row| row.source_row >= requested_top)
                .unwrap_or(cursor_row);
        }
        self.top = self
            .top
            .min(cursor_row)
            .max(cursor_row.saturating_sub(usize::from(area.height.saturating_sub(1))));
        let visible = &rows[self.top..rows.len().min(self.top + usize::from(area.height))];
        if let (Some(first), Some(last)) = (visible.first(), visible.last()) {
            // Keep native logical operations anchored while our viewport uses display rows.
            self.source_top = first.source_row;
            state.set_viewport_offset(0, first.source_row);
            state.set_viewport_height(last.source_row - first.source_row + 1);
        }
        buffer.set_style(area, Style::default().fg(palette.text));
        for (offset, row) in visible.iter().enumerate() {
            let y = area.y + u16::try_from(offset).unwrap_or_default();
            row.line
                .clone()
                .render(Rect::new(area.x, y, area.width, 1), buffer);
            if let Some(column) = row.cursor {
                buffer[(area.x + u16::try_from(column).unwrap_or_default(), y)]
                    .set_style(Style::default().fg(palette.cursor).bg(palette.focus));
            }
        }
    }
}

impl EditorRow {
    fn document(state: &EditorState, width: u16, palette: Palette) -> Vec<Self> {
        let mut rows = state
            .lines
            .iter_row()
            .enumerate()
            .flat_map(|(row, chars)| Self::wrap(state, row, chars, width, palette))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            rows.extend(Self::wrap(state, 0, &[], width, palette));
        }
        rows
    }

    fn wrap(
        state: &EditorState,
        source_row: usize,
        chars: &[char],
        width: u16,
        palette: Palette,
    ) -> Vec<Self> {
        let width = usize::from(width.max(1));
        let text = chars.iter().collect::<String>();
        let max_col = chars
            .len()
            .saturating_sub(usize::from(state.mode != EditorMode::Insert));
        let cursor = (source_row == state.cursor.row).then_some(state.cursor.col.min(max_col));
        let search = Self::search_selection(state, source_row, chars);
        let mut column = 0;
        let mut glyphs = Vec::new();
        for symbol in text.graphemes(true) {
            let end = column + symbol.chars().count();
            let selected = (column..end).any(|col| {
                state
                    .selection
                    .as_ref()
                    .is_some_and(|selection| selection.contains(&Index2::new(source_row, col)))
            });
            glyphs.push(EditorGlyph::new(
                symbol,
                width,
                selected || search.as_ref().is_some_and(|range| range.contains(&column)),
                cursor.is_some_and(|cursor| (column..end).contains(&cursor)),
                palette,
                column,
            ));
            column = end;
        }
        if glyphs.is_empty() {
            return vec![Self {
                line: Line::default(),
                source_row,
                cursor,
                positions: vec![(0, 0)],
            }];
        }
        let mut rows = Vec::new();
        let mut start = 0;
        while start < glyphs.len() {
            let end = EditorGlyph::wrap_end(&glyphs, start, width);
            let mut column = 0;
            let mut cursor = None;
            let mut spans = Vec::new();
            let mut positions = Vec::new();
            for glyph in &glyphs[start..end] {
                if column < width {
                    positions.push((column, glyph.source_column));
                }
                if glyph.cursor {
                    cursor = Some(column.min(width - 1));
                }
                spans.push(glyph.span.clone());
                column += glyph.width;
            }
            rows.push(Self {
                line: Line::from(spans),
                source_row,
                cursor,
                positions,
            });
            start = end;
        }
        if state.mode == EditorMode::Insert {
            Self::place_end_cursor(
                &mut rows,
                source_row,
                chars.len(),
                width,
                cursor == Some(chars.len()),
            );
        }
        rows
    }

    fn place_end_cursor(
        rows: &mut Vec<Self>,
        source_row: usize,
        source_column: usize,
        width: usize,
        has_cursor: bool,
    ) {
        let Some(last) = rows.last_mut() else { return };
        let column = last.line.width();
        if column < width {
            last.positions.push((column, source_column));
            if has_cursor {
                last.cursor = Some(column);
            }
        } else if has_cursor {
            rows.push(Self {
                line: Line::default(),
                source_row,
                cursor: Some(0),
                positions: vec![(0, source_column)],
            });
        }
    }

    fn search_selection(state: &EditorState, row: usize, chars: &[char]) -> Option<Range<usize>> {
        if state.mode != EditorMode::Search || row != state.cursor.row {
            return None;
        }
        let pattern = state.search_pattern().chars().collect::<Vec<_>>();
        let start = state.cursor.col;
        (!pattern.is_empty()
            && chars
                .get(start..)
                .is_some_and(|suffix| suffix.starts_with(&pattern)))
        .then_some(start..start + pattern.len())
    }
}

impl EditorGlyph {
    fn new(
        symbol: &str,
        width: usize,
        selected: bool,
        cursor: bool,
        palette: Palette,
        source_column: usize,
    ) -> Self {
        let text = match symbol {
            "\t" => " ".repeat(2.min(width)),
            _ if symbol.width() > width => "�".to_owned(),
            _ => symbol.to_owned(),
        };
        let mut style = Style::default().fg(palette.text);
        if selected {
            style = style.bg(palette.selection);
        }
        Self {
            width: text.width(),
            span: Span::styled(text, style),
            whitespace: symbol.chars().all(char::is_whitespace),
            cursor,
            source_column,
        }
    }

    fn wrap_end(glyphs: &[Self], start: usize, width: usize) -> usize {
        let mut end = start;
        let mut used = 0;
        let mut boundary = None;
        while let Some(glyph) = glyphs.get(end) {
            if used + glyph.width > width && end > start {
                break;
            }
            used += glyph.width;
            end += 1;
            if glyph.whitespace {
                boundary = Some(end);
            }
        }
        if end < glyphs.len() && !glyphs[end].whitespace {
            end = boundary.unwrap_or(end);
        }
        // Trim overflowing break spaces unless they contain the cursor: then keep
        // those spaces on visible rows so the caret still identifies the edited character.
        let mut after_spaces = end;
        while glyphs
            .get(after_spaces)
            .is_some_and(|glyph| glyph.whitespace)
        {
            if glyphs[after_spaces].cursor {
                return end;
            }
            after_spaces += 1;
        }
        after_spaces
    }
}
