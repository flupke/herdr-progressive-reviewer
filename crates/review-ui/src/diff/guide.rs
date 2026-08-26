use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use review_guide::{GuideItem, GuideItemStatus, GuideTarget};
use review_state::ReviewStatus;

use crate::app::{GuideCounter, ReviewFile};

use super::{DiffView, DiffViewport, WrappedDiffRow, wrap_line};

struct PositionedGuide<'a> {
    item: &'a GuideItem,
    first_row: usize,
    last_row: usize,
    counter: GuideCounter,
}

#[derive(Clone, Copy)]
pub(super) struct GuideStart<'a> {
    text: &'a str,
    target_row: usize,
    is_file_target: bool,
    status: GuideItemStatus,
    counter: GuideCounter,
}

#[derive(Clone, Copy)]
pub(super) struct GuideEnd {
    target_row: usize,
    status: GuideItemStatus,
}

#[derive(Clone, Copy)]
enum GuideRuleKind {
    Top(GuideCounter),
    Middle,
    Bottom,
}

pub(crate) enum GuideTargetPosition {
    Unloaded,
    Rows { first: usize },
}

#[derive(Default)]
pub(super) struct GuideRow<'a> {
    pub(super) starts: Vec<GuideStart<'a>>,
    pub(super) enclosing_status: Option<GuideItemStatus>,
    pub(super) ends: Vec<GuideEnd>,
}

#[derive(Clone, Copy)]
pub(super) struct GuideBorderCell {
    pub(super) column: usize,
    pub(super) symbol: char,
    pub(super) status: GuideItemStatus,
}

impl<'a> PositionedGuide<'a> {
    fn start(&self) -> GuideStart<'a> {
        GuideStart {
            text: &self.item.text,
            target_row: self.first_row,
            is_file_target: matches!(self.item.target, GuideTarget::File { .. }),
            status: self.item.status,
            counter: self.counter,
        }
    }

    fn end(&self) -> Option<GuideEnd> {
        (!matches!(self.item.target, GuideTarget::File { .. })).then_some(GuideEnd {
            target_row: self.first_row,
            status: self.item.status,
        })
    }
}

impl GuideStart<'_> {
    pub(super) fn append(
        self,
        view: &DiffView<'_>,
        rows: &mut Vec<WrappedDiffRow>,
        width: u16,
        line_number_width: usize,
    ) {
        rows.push(DiffView::guide_rule(
            GuideRuleKind::Top(self.counter),
            width,
            line_number_width,
            self.target_row,
            self.status,
        ));
        rows.extend(view.guide_text_rows(
            self.text,
            width,
            line_number_width,
            self.target_row,
            self.status,
        ));
        let rule = if self.is_file_target {
            GuideRuleKind::Bottom
        } else {
            GuideRuleKind::Middle
        };
        rows.push(DiffView::guide_rule(
            rule,
            width,
            line_number_width,
            self.target_row,
            self.status,
        ));
    }
}

impl GuideEnd {
    pub(super) fn append(
        self,
        rows: &mut Vec<WrappedDiffRow>,
        width: u16,
        line_number_width: usize,
    ) {
        rows.push(DiffView::guide_rule(
            GuideRuleKind::Bottom,
            width,
            line_number_width,
            self.target_row,
            self.status,
        ));
    }
}

impl DiffViewport {
    pub(crate) fn guide_start_visual_row(&self, source_row: usize) -> Option<usize> {
        self.rows
            .iter()
            .position(|row| row.source_row == source_row)
    }
}

impl DiffView<'_> {
    fn positioned_guides<'a>(&'a self, file: &ReviewFile) -> Vec<PositionedGuide<'a>> {
        if file.status == ReviewStatus::Reviewed {
            return Vec::new();
        }
        self.0
            .guide_items
            .iter()
            .enumerate()
            .filter_map(|(item_index, item)| {
                Self::guide_target_rows(file, &item.target).and_then(|(first_row, last_row)| {
                    let counter = self
                        .0
                        .guide_item_counters
                        .get(item_index)
                        .copied()
                        .flatten()?;
                    (first_row <= last_row && last_row < file.diff.rows.len()).then_some(
                        PositionedGuide {
                            item,
                            first_row,
                            last_row,
                            counter,
                        },
                    )
                })
            })
            .collect()
    }

    pub(super) fn guide_rows<'a>(&'a self, file: &ReviewFile) -> Vec<GuideRow<'a>> {
        let mut rows = std::iter::repeat_with(GuideRow::default)
            .take(file.diff.rows.len())
            .collect::<Vec<_>>();
        for guide in self.positioned_guides(file) {
            rows[guide.first_row].starts.push(guide.start());
            for row in &mut rows[guide.first_row..=guide.last_row] {
                row.enclosing_status.get_or_insert(guide.item.status);
            }
            if let Some(end) = guide.end() {
                rows[guide.last_row].ends.push(end);
            }
        }
        rows
    }

    pub(crate) fn guide_target_rows(
        file: &ReviewFile,
        target: &GuideTarget,
    ) -> Option<(usize, usize)> {
        match target {
            GuideTarget::Hunks {
                path,
                first_hunk,
                last_hunk,
            } if path == &file.path => file.diff.changed_rows_for_hunks(*first_hunk, *last_hunk),
            GuideTarget::Lines { path, old, new } if path == &file.path => {
                file.diff.rows_for_line_target(old.as_ref(), new.as_ref())
            }
            GuideTarget::File { path } if path == &file.path => Some((0, 0)),
            GuideTarget::Hunks { .. } | GuideTarget::Lines { .. } | GuideTarget::File { .. } => {
                None
            }
        }
    }

    pub(crate) fn guide_target_position(
        file: &ReviewFile,
        target: &GuideTarget,
    ) -> Option<GuideTargetPosition> {
        if file.diff.is_empty() && !file.diff.can_show_file() {
            return Some(GuideTargetPosition::Unloaded);
        }
        let (first, last) = Self::guide_target_rows(file, target)?;
        (first <= last && last < file.diff.rows.len())
            .then_some(GuideTargetPosition::Rows { first })
    }

    pub(super) fn reserve_guide_edges(
        line: &mut Line<'static>,
        width: u16,
        line_number_width: usize,
        status: GuideItemStatus,
    ) -> Vec<GuideBorderCell> {
        let available = usize::from(width).saturating_sub(line.width());
        if available > 0 {
            line.spans.push(Span::raw(" ".repeat(available)));
        }
        Self::guide_edge_cells(width, line_number_width, status)
    }

    fn guide_edge_cells(
        width: u16,
        line_number_width: usize,
        status: GuideItemStatus,
    ) -> Vec<GuideBorderCell> {
        let width = usize::from(width);
        let left = line_number_width + 2;
        let mut cells = Vec::new();
        if left < width {
            cells.push(GuideBorderCell {
                column: left,
                symbol: '│',
                status,
            });
        }
        if let Some(right) = width.checked_sub(1).filter(|right| *right != left) {
            cells.push(GuideBorderCell {
                column: right,
                symbol: '│',
                status,
            });
        }
        cells
    }

    pub(super) fn guide_style(&self, status: GuideItemStatus) -> Style {
        let style = Style::default().fg(self.0.palette.guide);
        if status == GuideItemStatus::Stale {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    }

    fn guide_rule(
        kind: GuideRuleKind,
        width: u16,
        line_number_width: usize,
        target_row: usize,
        status: GuideItemStatus,
    ) -> WrappedDiffRow {
        let (left, right, counter) = match kind {
            GuideRuleKind::Top(counter) => (
                '╭',
                '╮',
                Some(format!(" {}/{} ", counter.number, counter.total)),
            ),
            GuideRuleKind::Middle => ('├', '┤', None),
            GuideRuleKind::Bottom => ('╰', '╯', None),
        };
        let prefix = format!("  {:width$}", "", width = line_number_width);
        let rule_width = usize::from(width).saturating_sub(prefix.len());
        let counter_width = counter.as_ref().map_or(0, String::len);
        let rule = if rule_width >= counter_width.saturating_add(2) {
            format!(
                "{left}{}{counter}{right}",
                "─".repeat(rule_width - counter_width - 2),
                counter = counter.as_deref().unwrap_or_default(),
            )
        } else {
            left.to_string()
        };
        let line = Line::raw(" ".repeat(usize::from(width)));
        let guide_border_cells = rule
            .chars()
            .enumerate()
            .map(|(offset, symbol)| GuideBorderCell {
                column: prefix.len() + offset,
                symbol,
                status,
            })
            .collect();
        WrappedDiffRow {
            line,
            guide_border_cells,
            source_row: target_row,
            source_display_offset: 0,
            is_source_row: false,
        }
    }

    fn guide_text_rows(
        &self,
        text: &str,
        width: u16,
        line_number_width: usize,
        target_row: usize,
        status: GuideItemStatus,
    ) -> Vec<WrappedDiffRow> {
        let prefix = format!("  {:width$}   ", "", width = line_number_width);
        let guide_style = self.guide_style(status);
        let guide = Line::from(vec![
            Span::styled(prefix, guide_style),
            Span::styled(text.to_owned(), guide_style),
        ]);
        wrap_line(&guide, width.saturating_sub(1), line_number_width + 5)
            .into_iter()
            .map(|(mut line, _)| {
                let guide_border_cells =
                    Self::reserve_guide_edges(&mut line, width, line_number_width, status);
                WrappedDiffRow {
                    line,
                    guide_border_cells,
                    source_row: target_row,
                    source_display_offset: 0,
                    is_source_row: false,
                }
            })
            .collect()
    }
}
