use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::{GuideBorderCell, GuideRenderedRow, wrap_line};

/// A gutter-aligned frame shared by guides and review conversations.
#[derive(Clone, Copy)]
pub struct DiffFrame {
    width: u16,
    left: usize,
    padding: usize,
    style: Style,
}

/// The horizontal boundaries of an inline diff frame.
#[derive(Clone, Copy)]
pub enum FrameRule<'a> {
    Top(Option<&'a str>),
    Middle,
    Bottom,
}

impl DiffFrame {
    pub fn new(width: u16, line_number_width: usize, style: Style) -> Self {
        Self {
            width,
            left: line_number_width + 2,
            padding: 2,
            style,
        }
    }

    /// Set the horizontal padding inside the frame's content borders.
    #[must_use]
    pub fn with_padding(self, padding: u16) -> Self {
        Self {
            padding: usize::from(padding),
            ..self
        }
    }

    /// Width available to content after the gutter and frame padding.
    pub fn content_width(self) -> u16 {
        self.width
            .saturating_sub(u16::try_from(self.left + 2 + self.padding * 2).unwrap_or(u16::MAX))
    }

    /// First content column after the gutter, border, and padding.
    pub fn content_column(self) -> usize {
        self.left + 1 + self.padding
    }

    /// Frame an existing diff line without replacing its colors.
    pub fn enclose_line(self, line: &mut Line<'static>) -> Vec<GuideBorderCell> {
        let width = usize::from(self.width);
        let available = width.saturating_sub(line.width());
        if available > 0 {
            line.spans.push(Span::raw(" ".repeat(available)));
        }
        let mut cells = Vec::new();
        if self.left < width {
            cells.push(self.cell(self.left, '│'));
        }
        if let Some(right) = width.checked_sub(1).filter(|right| *right != self.left) {
            cells.push(self.cell(right, '│'));
        }
        cells
    }

    pub fn rule(self, kind: FrameRule<'_>, source_row: usize) -> GuideRenderedRow {
        let (left, right, label) = match kind {
            FrameRule::Top(label) => ('╭', '╮', label.unwrap_or_default()),
            FrameRule::Middle => ('├', '┤', ""),
            FrameRule::Bottom => ('╰', '╯', ""),
        };
        let rule_width = usize::from(self.width).saturating_sub(self.left);
        let label_width = Line::raw(label).width();
        let rule = if rule_width >= label_width.saturating_add(2) {
            format!(
                "{left}{}{label}{right}",
                "─".repeat(rule_width - label_width - 2)
            )
        } else {
            left.to_string()
        };
        GuideRenderedRow {
            line: Line::raw(" ".repeat(usize::from(self.width))),
            border_cells: rule
                .chars()
                .enumerate()
                .filter(|(offset, _)| self.left + offset < usize::from(self.width))
                .map(|(offset, symbol)| self.cell(self.left + offset, symbol))
                .collect(),
            source_row,
        }
    }

    /// Wrap text inside the same padded content columns as review guides.
    pub(super) fn text(self, text: &str, style: Style, source_row: usize) -> Vec<GuideRenderedRow> {
        let indent = self.content_column();
        let content = Line::from(vec![
            Span::styled(" ".repeat(indent), style),
            Span::styled(text.to_owned(), style),
        ]);
        wrap_line(&content, self.width.saturating_sub(1), indent)
            .into_iter()
            .map(|mut line| GuideRenderedRow {
                border_cells: self.enclose_line(&mut line),
                line,
                source_row,
            })
            .collect()
    }

    /// Place a rendered line inside the padded content columns, preserving its style.
    fn content(self, mut line: Line<'static>, source_row: usize) -> GuideRenderedRow {
        let style = std::mem::take(&mut line.style);
        for span in &mut line.spans {
            span.style = style.patch(span.style);
        }
        line.spans.insert(0, Span::raw(" ".repeat(self.left + 1)));
        line.spans
            .insert(1, Span::styled(" ".repeat(self.padding), style));
        let padding = usize::from(self.width.saturating_sub(1)).saturating_sub(line.width());
        line.spans.push(Span::styled(" ".repeat(padding), style));
        GuideRenderedRow {
            border_cells: self.enclose_line(&mut line),
            line,
            source_row,
        }
    }

    /// Wrap a styled content line without coloring the diff gutter.
    pub fn wrapped_content(self, line: &Line<'static>, source_row: usize) -> Vec<GuideRenderedRow> {
        wrap_line(line, self.content_width(), 0)
            .into_iter()
            .map(|wrapped| self.content(wrapped.style(line.style), source_row))
            .collect()
    }

    fn cell(self, column: usize, symbol: char) -> GuideBorderCell {
        GuideBorderCell {
            column,
            symbol,
            style: self.style,
        }
    }
}
