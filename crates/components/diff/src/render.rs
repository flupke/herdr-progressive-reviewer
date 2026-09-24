use std::ops::{Range, RangeInclusive};
#[path = "evidence.rs"]
mod evidence;
use evidence::{EvidenceFrames, RequiredEvidenceRows};

use guide_rendering::{
    DiffFrame, GuideBorderCell, GuideLayout, GuideOverlay, GuideOverlayRow, GuideRenderedRow,
};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use review_lsp::SourceLocation;
use review_repository::diff::{DiffRow, NoticeKind};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use ui_theme::Palette;

use crate::comment_layout::CommentLayout;
use crate::{DiffPointerPosition, LoadedDocument, PresentedRow, Token};

pub(super) const TAB_DISPLAY_WIDTH: usize = 4;
const DIFF_CONTROLS_TITLE: &str = "[←→] [→←] [👁 ]";
const BASIC_DIFF_CONTROLS_TITLE: &str = "[←→] [→←]";
const FILE_CONTROL_TITLE: &str = "[x]";
const MIN_DIFF_CONTROLS_WIDTH: u16 = 32;

pub(super) struct DiffRenderer<'a> {
    comments: Option<&'a crate::comments::Comments>,
    evidence: Option<&'a crate::explore::ExploreView>,
    palette: Palette,
    file: Option<&'a LoadedDocument>,
    guide_layout: Option<GuideLayout<'a>>,
    focused: bool,
    reviewable: bool,
    search_query: Option<&'a str>,
    search_pattern: text_search::Query,
    selection: Option<RangeInclusive<usize>>,
}

impl<'a> DiffRenderer<'a> {
    pub(super) fn original_context_rows(
        rows: &[syntax_highlighting::HighlightedRow],
        width: u16,
        number_width: usize,
        palette: Palette,
    ) -> Vec<crate::comment_layout::CommentRow> {
        let renderer = Self::new(palette, None, None, false, false, None, None);
        let frame = DiffFrame::new(width, number_width, Style::default().fg(palette.focus));
        rows.iter()
            .enumerate()
            .flat_map(|(index, row)| {
                let context = CodeRenderContext {
                    tokens: &row.tokens,
                    number_width,
                    cursor: None,
                    source_line: None,
                    source_location: None,
                };
                let line = renderer
                    .diff_line(&row.diff, context, true)
                    .style(renderer.row_style(&row.diff, true));
                WrappedDiffRow::wrap_source(
                    &line,
                    index,
                    width,
                    number_width + 3,
                    Some(frame),
                    None,
                )
                .into_iter()
                .map(|row| crate::comment_layout::CommentRow {
                    rendered: GuideRenderedRow {
                        line: row.line,
                        border_cells: row.guide_border_cells,
                        source_row: row.source_row,
                    },
                    target: None,
                    editor: false,
                    reply: None,
                })
            })
            .collect()
    }

    pub(super) fn with_evidence(mut self, evidence: &'a crate::explore::ExploreView) -> Self {
        self.evidence = Some(evidence);
        self
    }

    pub(super) fn with_comments(mut self, comments: &'a crate::comments::Comments) -> Self {
        self.comments = Some(comments);
        self
    }

    pub(super) fn new(
        palette: Palette,
        file: Option<&'a LoadedDocument>,
        guide_layout: Option<GuideLayout<'a>>,
        focused: bool,
        reviewable: bool,
        search_query: Option<&'a str>,
        selection: Option<RangeInclusive<usize>>,
    ) -> Self {
        Self {
            comments: None,
            evidence: None,
            palette,
            file,
            guide_layout,
            focused,
            reviewable,
            search_query,
            search_pattern: text_search::Query::new(search_query.unwrap_or_default()),
            selection,
        }
    }
}

#[derive(Clone, Copy)]
struct CodeRenderContext<'a> {
    tokens: &'a [Token],
    number_width: usize,
    cursor: Option<usize>,
    source_line: Option<u32>,
    source_location: Option<&'a SourceLocation>,
}

/// Visual row mapping for one rendered diff viewport.
pub(super) struct DiffViewport {
    rows: Vec<WrappedDiffRow>,
}

pub(super) struct DiffRenderResult {
    pub(super) guide_overlay: GuideOverlay,
    pub(super) pointer_viewport: Option<DiffPointerViewport>,
}

#[derive(Default)]
pub(super) struct DiffPointerViewport {
    rows: Vec<PointerRow>,
    line_number_width: usize,
}

struct PointerRow {
    comment: Option<crate::comments::CommentTarget>,
    editor: bool,
    source_row: usize,
    source_display_offset: usize,
    is_source_row: bool,
}

struct WrappedDiffRow {
    reply: Option<review_threads::MessageId>,
    comment: Option<crate::comments::CommentTarget>,
    editor: bool,
    line: Line<'static>,
    guide_line: Option<Line<'static>>,
    guide_border_cells: Vec<GuideBorderCell>,
    source_row: usize,
    source_display_offset: usize,
    is_source_row: bool,
}

fn wrapped_guide_row(row: GuideRenderedRow) -> WrappedDiffRow {
    WrappedDiffRow {
        comment: None,
        editor: false,
        reply: None,
        line: Line::raw(" ".repeat(row.line.width())),
        guide_line: Some(row.line),
        guide_border_cells: row.border_cells,
        source_row: row.source_row,
        source_display_offset: 0,
        is_source_row: false,
    }
}

fn append_guide_rows_before(
    rows: &mut Vec<WrappedDiffRow>,
    layout: &GuideLayout<'_>,
    source_row: usize,
    width: u16,
    line_number_width: usize,
) {
    rows.extend(
        layout
            .rows_before(source_row, width, line_number_width)
            .into_iter()
            .map(wrapped_guide_row),
    );
}

fn append_guide_rows_after(
    rows: &mut Vec<WrappedDiffRow>,
    layout: &GuideLayout<'_>,
    source_row: usize,
    width: u16,
    line_number_width: usize,
) {
    rows.extend(
        layout
            .rows_after(source_row, width, line_number_width)
            .into_iter()
            .map(wrapped_guide_row),
    );
}

impl DiffViewport {
    fn comments_only(layout: Option<CommentLayout>) -> Self {
        Self {
            rows: layout
                .into_iter()
                .flat_map(CommentLayout::into_remaining)
                .map(WrappedDiffRow::from_comment)
                .collect(),
        }
    }

    pub(super) fn comment_range(
        &self,
        id: Option<&review_threads::MessageId>,
        editing: bool,
    ) -> Option<RangeInclusive<usize>> {
        let mut rows = self.rows.iter().enumerate().filter_map(|(index, row)| {
            let matches = if editing {
                row.editor
            } else {
                id.is_some()
                    && row
                        .comment
                        .as_ref()
                        .and_then(crate::comments::CommentTarget::id)
                        == id
            };
            matches.then_some(index)
        });
        let start = rows.next()?;
        Some(start..=rows.next_back().unwrap_or(start))
    }

    pub(super) fn scroll(&self, file: &LoadedDocument) -> usize {
        file.document.scroll.min(self.rows.len().saturating_sub(1))
    }

    pub(super) fn scroll_with_cursor_visible(&self, file: &LoadedDocument, height: usize) -> usize {
        let last = self.rows.len().saturating_sub(height);
        let mut scroll = file.document.scroll.min(last);
        let cursor = self.cursor_visual_row(file);
        if cursor < scroll {
            scroll = cursor;
        } else if cursor >= scroll.saturating_add(height) {
            scroll = cursor + 1 - height;
        }
        scroll
    }

    pub(super) fn scroll_with_cursor_centered(
        &self,
        file: &LoadedDocument,
        height: usize,
    ) -> usize {
        self.cursor_visual_row(file)
            .saturating_sub(height / 2)
            .min(self.rows.len().saturating_sub(height))
    }

    pub(super) fn scroll_with_guide_top_aligned(&self, file: &LoadedDocument) -> usize {
        self.rows
            .iter()
            .position(|row| row.source_row == file.document.cursor)
            .unwrap_or_else(|| self.cursor_visual_row(file))
    }

    pub(super) fn source_row_at(&self, visual_row: usize) -> Option<usize> {
        self.rows.get(visual_row).map(|row| row.source_row)
    }

    pub(super) fn source_column_at(
        &self,
        visual_row: usize,
        pane_column: usize,
        number_width: usize,
    ) -> Option<usize> {
        let row = self.rows.get(visual_row)?;
        if !row.is_source_row {
            return None;
        }
        Some(
            row.source_display_offset
                .saturating_add(pane_column.saturating_sub(number_width + 3)),
        )
    }

    pub(super) fn source_position_after_visual_delta(
        &self,
        file: &LoadedDocument,
        delta: isize,
    ) -> Option<(usize, usize)> {
        let visual_row = self
            .cursor_visual_row(file)
            .saturating_add_signed(delta)
            .min(self.rows.len().saturating_sub(1));
        let index = if delta >= 0 {
            (visual_row..self.rows.len())
                .find(|index| self.rows[*index].comment.is_none() && !self.rows[*index].editor)
        } else {
            (0..=visual_row)
                .rev()
                .find(|index| self.rows[*index].comment.is_none() && !self.rows[*index].editor)
        }?;
        let row = self.rows.get(index)?;
        Some((row.source_row, row.source_display_offset))
    }

    pub(super) fn visible_row_count(&self) -> usize {
        self.rows.len()
    }

    pub(super) fn visible_cursor_position(
        &self,
        file: &LoadedDocument,
        height: usize,
    ) -> Option<DiffPointerPosition> {
        let cursor = self.cursor_visual_row(file);
        let scroll = self.scroll(file);
        let visible = scroll..scroll.saturating_add(height).min(self.rows.len());
        if visible.contains(&cursor) {
            return None;
        }
        let target = visible
            .filter(|index| self.rows[*index].is_source_row)
            .min_by_key(|index| index.abs_diff(cursor))?;
        Some(DiffPointerPosition {
            row: self.rows[target].source_row,
            column: Some(self.column_on_row(file, cursor, target)),
        })
    }

    fn column_on_row(&self, file: &LoadedDocument, cursor: usize, target: usize) -> usize {
        let row = &self.rows[target];
        let column = Self::cursor_display_column(file)
            .saturating_sub(self.rows[cursor].source_display_offset);
        let end = self
            .rows
            .get(target + 1)
            .filter(|next| next.is_source_row && next.source_row == row.source_row)
            .map_or_else(
                || {
                    file.document
                        .diff
                        .source_position(row.source_row)
                        .map_or(0, |(_, line)| source_display_width(&line, line.len()))
                },
                |next| next.source_display_offset,
            );
        row.source_display_offset
            .saturating_add(column)
            .min(end.saturating_sub(1))
            .max(row.source_display_offset)
    }

    fn cursor_display_column(file: &LoadedDocument) -> usize {
        file.document
            .diff
            .source_position(file.document.cursor)
            .map_or(0, |(_, line)| {
                source_display_width(&line, file.document.column)
            })
    }

    pub(super) fn cursor_visual_row(&self, file: &LoadedDocument) -> usize {
        let source_display_column = Self::cursor_display_column(file);
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.is_source_row
                    && row.source_row == file.document.cursor
                    && row.source_display_offset <= source_display_column
            })
            .map(|(index, _)| index)
            .next_back()
            .or_else(|| {
                self.rows
                    .iter()
                    .position(|row| row.source_row == file.document.cursor)
            })
            .unwrap_or(0)
    }
}

impl DiffPointerViewport {
    pub(super) fn comment_at(&self, screen_row: usize) -> Option<&crate::comments::CommentTarget> {
        self.rows
            .get(screen_row)
            .and_then(|row| row.comment.as_ref())
    }

    pub(super) fn is_comment(&self, screen_row: usize) -> bool {
        self.rows
            .get(screen_row)
            .is_some_and(|row| row.editor || row.comment.is_some())
    }

    pub(super) fn position(
        &self,
        screen_row: usize,
        pane_column: usize,
    ) -> Option<(usize, Option<usize>)> {
        let row = self.rows.get(screen_row)?;
        let column = row.is_source_row.then(|| {
            row.source_display_offset.saturating_add(
                pane_column.saturating_sub(self.line_number_width.saturating_add(3)),
            )
        });
        Some((row.source_row, column))
    }
}

impl DiffRenderer<'_> {
    pub(super) fn render(
        self,
        area: Rect,
        buffer: &mut Buffer,
        replies: &mut crate::reply_visibility::ReplyVisibility,
    ) -> DiffRenderResult {
        let focused = self.focused;
        let file = self.file;
        let inner = self.render_pane(area, buffer, focused, file);
        let Some(file) = file else {
            return DiffRenderResult {
                guide_overlay: GuideOverlay::empty(),
                pointer_viewport: None,
            };
        };
        if self.render_empty_review(file, inner, buffer) {
            return DiffRenderResult {
                guide_overlay: GuideOverlay::empty(),
                pointer_viewport: None,
            };
        }
        let viewport = self.viewport(file, inner.width, focused);
        let scroll = viewport.scroll(file);
        replies.observe(
            viewport
                .rows
                .iter()
                .map(|row| (row.reply.as_ref(), &row.line)),
            scroll..scroll.saturating_add(usize::from(inner.height)),
            inner,
        );
        let visible_rows = viewport
            .rows
            .iter()
            .skip(scroll)
            .take(usize::from(inner.height))
            .collect::<Vec<_>>();
        let pointer_viewport = DiffPointerViewport {
            rows: visible_rows
                .iter()
                .map(|row| PointerRow {
                    comment: row.comment.clone(),
                    editor: row.editor,
                    source_row: row.source_row,
                    source_display_offset: row.source_display_offset,
                    is_source_row: row.is_source_row,
                })
                .collect(),
            line_number_width: file.document.diff.line_number_width(),
        };
        let lines = visible_rows
            .iter()
            .map(|row| row.line.clone())
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
        DiffRenderResult {
            guide_overlay: GuideOverlay::new(
                inner,
                visible_rows
                    .into_iter()
                    .enumerate()
                    .filter_map(|(row, visible)| {
                        let row = u16::try_from(row).ok()?;
                        (!visible.guide_border_cells.is_empty() || visible.guide_line.is_some())
                            .then(|| GuideOverlayRow {
                                row,
                                line: visible.guide_line.clone(),
                                border_cells: visible.guide_border_cells.clone(),
                            })
                    })
                    .collect(),
            ),
            pointer_viewport: Some(pointer_viewport),
        }
    }
}

impl DiffRenderer<'_> {
    fn render_pane(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        focused: bool,
        file: Option<&LoadedDocument>,
    ) -> Rect {
        let title = file.map_or_else(
            || "Diff".to_owned(),
            |file| {
                let kind = if file.document.diff.is_file_view() {
                    "File"
                } else {
                    "Diff"
                };
                format!("{kind} · {}", file.display_path)
            },
        );
        let controls = diff_control_title(file);
        let show_controls = diff_controls_are_visible(area.width, file);
        let title = if show_controls {
            shorten(
                &title,
                usize::from(area.width).saturating_sub(
                    controls.width() + 5 + if focused { " (focus)".len() } else { 0 },
                ),
            )
        } else {
            title
        };
        let mut block = pane_block(self.palette, &title, focused);
        if show_controls {
            block = block.title(
                Line::styled(controls, Style::default().fg(self.palette.focus))
                    .alignment(Alignment::Right),
            );
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        inner
    }

    fn render_empty_review(&self, file: &LoadedDocument, area: Rect, buffer: &mut Buffer) -> bool {
        if !file.comments_only
            && self.comments.is_none_or(|comments| !comments.has_for(file))
            && self.hides_reviewed_diff(file)
        {
            let center = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
            Paragraph::new("No changes")
                .style(Style::default().fg(self.palette.dim))
                .alignment(Alignment::Center)
                .render(center, buffer);
            return true;
        }
        false
    }

    fn hides_reviewed_diff(&self, file: &LoadedDocument) -> bool {
        !self.reviewable
            && self.search_query.is_none()
            && file.document.source_location.is_none()
            && !file.document.diff.is_file_view()
    }

    pub(super) fn viewport(
        &self,
        file: &LoadedDocument,
        width: u16,
        focused: bool,
    ) -> DiffViewport {
        let evidence = EvidenceFrames::new(file, width, self.evidence);
        self.viewport_with_frames(file, width, focused, &evidence)
    }

    fn viewport_with_frames(
        &self,
        file: &LoadedDocument,
        width: u16,
        focused: bool,
        evidence: &EvidenceFrames,
    ) -> DiffViewport {
        let selection = self.selection.clone();
        let required_rows = RequiredEvidenceRows::new(file, self.evidence);
        let line_number_width = file.document.diff.line_number_width();
        let show_markers = !file.document.diff.shows_whole_file();
        let guide_layout = self.guide_layout.as_ref();
        let include_code = !self.hides_reviewed_diff(file);
        let comment_layout = self
            .comments
            .map(|comments| comments.layout(file, width, self.palette, include_code));
        if !include_code {
            return DiffViewport::comments_only(comment_layout);
        }
        let editing = self
            .comments
            .is_some_and(|comments| comments.inline_editor_visible_in(file));
        let focused = focused && !editing;
        let rows: Vec<_> = file
            .document
            .diff
            .rows
            .iter()
            .enumerate()
            .filter(|(index, _)| required_rows.as_ref().is_none_or(|rows| rows.shows(*index)))
            .flat_map(|(index, presented)| {
                let mut wrapped = Vec::new();
                if let Some(layout) = guide_layout {
                    append_guide_rows_before(&mut wrapped, layout, index, width, line_number_width);
                }
                let source_line = file
                    .document
                    .diff
                    .source_position(index)
                    .map(|(line, _)| line);
                let context = |tokens| CodeRenderContext {
                    tokens,
                    number_width: line_number_width,
                    cursor: (focused && index == file.document.cursor)
                        .then_some(file.document.column),
                    source_line,
                    source_location: file.document.source_location.as_ref(),
                };
                let (line, mut style) = match presented {
                    PresentedRow::Diff { source, tokens } => {
                        let row = file.document.diff.source_row(*source);
                        (
                            self.diff_line(row, context(tokens), show_markers),
                            self.row_style(row, show_markers),
                        )
                    }
                    PresentedRow::Gap { lines, .. } => (
                        Self::gap_line(lines.len(), line_number_width, usize::from(width)),
                        Style::default()
                            .fg(self.palette.text)
                            .bg(self.palette.selection),
                    ),
                    PresentedRow::Expanded { line, tokens } => (
                        self.code_line(Some(*line), None, context(tokens)),
                        Style::default().fg(self.palette.text),
                    ),
                };
                if selection
                    .as_ref()
                    .is_some_and(|selection| selection.contains(&index))
                {
                    style = style.bg(self.palette.selection);
                }
                let is_current_row = !editing && index == file.document.cursor;
                if is_current_row {
                    style = style.bg(self.palette.cursor);
                }
                let styled_line = line.style(style);
                let enclosing_status =
                    guide_layout.and_then(|layout| layout.enclosing_status(index));
                let comment_frame = comment_layout
                    .as_ref()
                    .and_then(|layout| layout.frame_at(index));
                let enclosing_frame = comment_frame.or_else(|| {
                    guide_layout
                        .zip(enclosing_status)
                        .map(|(layout, status)| layout.frame(width, line_number_width, status))
                });
                let enclosing_frame = enclosing_frame.or_else(|| evidence.frame_at(index));
                wrapped.extend(WrappedDiffRow::wrap_source(
                    &styled_line,
                    index,
                    width,
                    line_number_width + 3,
                    enclosing_frame,
                    is_current_row.then_some(self.palette.cursor),
                ));
                if let Some(layout) = guide_layout {
                    append_guide_rows_after(&mut wrapped, layout, index, width, line_number_width);
                }
                wrapped
            })
            .collect();
        let rows = Self::insert_comment_rows(rows, comment_layout);
        DiffViewport {
            rows: evidence.outline(rows),
        }
    }

    fn insert_comment_rows(
        source: Vec<WrappedDiffRow>,
        layout: Option<crate::comment_layout::CommentLayout>,
    ) -> Vec<WrappedDiffRow> {
        let Some(mut layout) = layout else {
            return source;
        };
        let mut source = source.into_iter().peekable();
        let mut rows = Vec::new();
        while let Some(row) = source.next() {
            let index = row.source_row;
            let is_source_row = row.is_source_row;
            if is_source_row {
                rows.extend(
                    layout
                        .take_before(index)
                        .into_iter()
                        .map(WrappedDiffRow::from_comment),
                );
            }
            rows.push(row);
            if is_source_row
                && source
                    .peek()
                    .is_none_or(|next| !next.is_source_row || next.source_row != index)
            {
                rows.extend(
                    layout
                        .take_after(index)
                        .into_iter()
                        .map(WrappedDiffRow::from_comment),
                );
            }
        }
        rows.extend(layout.into_remaining().map(WrappedDiffRow::from_comment));
        rows
    }

    fn diff_line(
        &self,
        row: &DiffRow,
        context: CodeRenderContext<'_>,
        show_markers: bool,
    ) -> Line<'static> {
        let (line, bar) = match row {
            DiffRow::Add { new_line, .. } => (
                Some(*new_line),
                show_markers.then_some(self.palette.insertion),
            ),
            DiffRow::Delete { old_line, .. } => (
                Some(*old_line),
                show_markers.then_some(self.palette.deletion),
            ),
            DiffRow::Context { new_line, .. } => (Some(*new_line), None),
            DiffRow::Notice { text, .. } => return Line::raw(text.clone()),
            DiffRow::FileHeader { .. } | DiffRow::Meta { .. } | DiffRow::Hunk { .. } => {
                return Line::default();
            }
        };
        self.code_line(line, bar, context)
    }

    fn code_line(
        &self,
        line: Option<u32>,
        bar: Option<Color>,
        context: CodeRenderContext<'_>,
    ) -> Line<'static> {
        let mut spans = vec![bar.map_or_else(
            || Span::raw("  "),
            |color| {
                Span::styled(
                    "▌ ",
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )
            },
        )];
        spans.push(Span::styled(
            line.map_or_else(
                || " ".repeat(context.number_width + 1),
                |line| format!("{line:>width$} ", width = context.number_width),
            ),
            Style::default().fg(self.palette.dim),
        ));
        spans.extend(self.code_spans(
            context.tokens,
            context.cursor,
            context.source_line,
            context.source_location,
        ));
        Line::from(spans)
    }

    fn code_spans(
        &self,
        tokens: &[Token],
        cursor_column: Option<usize>,
        source_line: Option<u32>,
        source_location: Option<&SourceLocation>,
    ) -> Vec<Span<'static>> {
        let text = tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<String>();
        let source_selection = source_line
            .zip(source_location)
            .and_then(|(line, location)| location.range_in_line(line, text.len()));
        let matches = self.search_pattern.ranges(&text).collect::<Vec<_>>();
        let tab_display = " ".repeat(TAB_DISPLAY_WIDTH);
        let mut spans = Vec::new();
        let mut token_start = 0;
        for token in tokens {
            let token_end = token_start + token.text.len();
            let boundaries = token_boundaries(
                token_start..token_end,
                &matches,
                source_selection.as_ref(),
                cursor_column,
                &text,
            );
            for pair in boundaries.windows(2) {
                let start = pair[0];
                let end = pair[1];
                let style = code_span_style(
                    token.color,
                    start..end,
                    &matches,
                    source_selection.as_ref(),
                    cursor_column,
                );
                spans.push(Span::styled(
                    token.text[start - token_start..end - token_start].replace('\t', &tab_display),
                    style,
                ));
            }
            token_start = token_end;
        }
        if cursor_column == Some(text.len()) {
            spans.push(Span::styled(
                " ",
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ));
        }
        spans
    }

    fn gap_line(count: usize, number_width: usize, width: usize) -> Line<'static> {
        let mut text = format!("  {:>number_width$} {count} unmodified lines", "…");
        text.push_str(&" ".repeat(width.saturating_sub(text.chars().count())));
        Line::raw(text)
    }

    fn row_style(&self, row: &DiffRow, show_markers: bool) -> Style {
        if !show_markers {
            return Style::default().fg(self.palette.text);
        }
        match row {
            DiffRow::Add { .. } => Style::default()
                .fg(self.palette.insertion)
                .bg(self.palette.insertion_bg),
            DiffRow::Delete { .. } => Style::default()
                .fg(self.palette.deletion)
                .bg(self.palette.deletion_bg),
            DiffRow::Notice {
                kind: NoticeKind::Binary,
                ..
            } => Style::default().fg(self.palette.focus),
            DiffRow::Notice {
                kind: NoticeKind::Conflict | NoticeKind::Unsupported,
                ..
            } => Style::default().fg(self.palette.warning),
            DiffRow::Meta { .. } | DiffRow::FileHeader { .. } | DiffRow::Hunk { .. } => {
                Style::default().fg(self.palette.dim)
            }
            DiffRow::Context { .. } => Style::default().fg(self.palette.text),
        }
    }
}

fn fill_line_background(line: &mut Line<'static>, width: u16, background: Color) {
    let remaining_width = usize::from(width).saturating_sub(line.width());
    if remaining_width > 0 {
        line.spans.push(Span::styled(
            " ".repeat(remaining_width),
            Style::default().bg(background),
        ));
    }
}

fn source_display_width(line: &str, byte_column: usize) -> usize {
    line.grapheme_indices(true)
        .take_while(|(byte, grapheme)| byte.saturating_add(grapheme.len()) <= byte_column)
        .map(|(_, grapheme)| {
            if grapheme == "\t" {
                TAB_DISPLAY_WIDTH
            } else {
                grapheme.width()
            }
        })
        .sum()
}

fn token_boundaries(
    token: Range<usize>,
    matches: &[Range<usize>],
    source_selection: Option<&Range<usize>>,
    cursor_column: Option<usize>,
    text: &str,
) -> Vec<usize> {
    let mut boundaries = vec![token.start, token.end];
    for range in matches
        .iter()
        .filter(|range| range.start < token.end && range.end > token.start)
    {
        boundaries.push(range.start.max(token.start));
        boundaries.push(range.end.min(token.end));
    }
    if let Some(range) =
        source_selection.filter(|range| range.start < token.end && range.end > token.start)
    {
        boundaries.push(range.start.max(token.start));
        boundaries.push(range.end.min(token.end));
    }
    if let Some(column) = cursor_column.filter(|column| {
        *column >= token.start && *column < token.end && text.is_char_boundary(*column)
    }) {
        boundaries.push(column);
        boundaries.push(
            (column + text[column..].graphemes(true).next().map_or(0, str::len)).min(token.end),
        );
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

fn code_span_style(
    color: Color,
    span: Range<usize>,
    matches: &[Range<usize>],
    source_selection: Option<&Range<usize>>,
    cursor_column: Option<usize>,
) -> Style {
    let selected = matches
        .iter()
        .any(|range| range.start <= span.start && range.end >= span.end)
        || source_selection.is_some_and(|range| range.start <= span.start && range.end >= span.end);
    let mut style = Style::default().fg(color);
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    if cursor_column == Some(span.start) {
        style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
    }
    style
}

#[derive(Clone)]
struct WrapGrapheme {
    span: Span<'static>,
    display_width: usize,
    is_whitespace: bool,
}

fn wrap_graphemes(line: &Line<'static>) -> Vec<WrapGrapheme> {
    line.styled_graphemes(Style::default())
        .map(|grapheme| {
            let symbol = grapheme.symbol.to_owned();
            WrapGrapheme {
                display_width: symbol.width(),
                is_whitespace: symbol == " " || symbol == "\t",
                span: Span::styled(symbol, grapheme.style),
            }
        })
        .collect()
}

fn find_wrap_end(
    graphemes: &[WrapGrapheme],
    start: usize,
    available_width: usize,
    consumed_width: usize,
    continuation_indent: usize,
) -> (usize, usize) {
    let mut end = start;
    let mut content_width = 0usize;
    let mut last_word_boundary = None;
    let mut saw_non_whitespace = false;
    while end < graphemes.len() {
        let grapheme = &graphemes[end];
        if end > start && content_width.saturating_add(grapheme.display_width) > available_width {
            break;
        }
        let grapheme_source_start = consumed_width.saturating_add(content_width);
        content_width = content_width.saturating_add(grapheme.display_width);
        end += 1;
        if grapheme_source_start >= continuation_indent {
            if grapheme.is_whitespace && saw_non_whitespace {
                last_word_boundary = Some(end);
            } else if !grapheme.is_whitespace {
                saw_non_whitespace = true;
            }
        }
    }
    if end < graphemes.len()
        && let Some(word_boundary) = last_word_boundary
    {
        end = word_boundary;
        content_width = graphemes[start..end]
            .iter()
            .map(|grapheme| grapheme.display_width)
            .sum();
    }
    (end, content_width)
}

fn continuation_prefix(
    graphemes: &[WrapGrapheme],
    start: usize,
    visible_indent: usize,
) -> Vec<Span<'static>> {
    if visible_indent == 0 {
        return Vec::new();
    }
    let marker = graphemes
        .first()
        .filter(|grapheme| grapheme.span.content == "▌");
    let mut spans = marker
        .map(|grapheme| vec![grapheme.span.clone()])
        .unwrap_or_default();
    let blank_width = visible_indent.saturating_sub(spans.len());
    if blank_width > 0 {
        spans.push(Span::styled(
            " ".repeat(blank_width),
            graphemes[start].span.style,
        ));
    }
    spans
}

fn wrap_line(
    line: &Line<'static>,
    width: u16,
    continuation_indent: usize,
) -> Vec<(Line<'static>, usize)> {
    let width = usize::from(width.max(1));
    let graphemes = wrap_graphemes(line);
    if graphemes.is_empty() {
        return vec![(Line::default(), 0)];
    }
    let mut wrapped = Vec::new();
    let mut start = 0;
    let mut consumed_width = 0usize;
    while start < graphemes.len() {
        let first_grapheme_width = graphemes[start].display_width;
        let visible_indent = if wrapped.is_empty() {
            0
        } else {
            continuation_indent.min(width.saturating_sub(first_grapheme_width))
        };
        let (end, content_width) = find_wrap_end(
            &graphemes,
            start,
            width.saturating_sub(visible_indent),
            consumed_width,
            continuation_indent,
        );
        let source_display_offset = consumed_width.saturating_sub(continuation_indent);
        let mut spans = continuation_prefix(&graphemes, start, visible_indent);
        spans.extend(
            graphemes[start..end]
                .iter()
                .map(|grapheme| grapheme.span.clone()),
        );
        wrapped.push((Line::from(spans), source_display_offset));
        consumed_width = consumed_width.saturating_add(content_width);
        start = end;
    }
    wrapped
}

fn pane_block(palette: Palette, title: &str, focused: bool) -> Block<'_> {
    let style = if focused {
        Style::default().fg(palette.focus)
    } else {
        Style::default().fg(palette.dim)
    };
    let suffix = if focused { " (focus)" } else { "" };
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title}{suffix} "))
        .border_style(style)
}

fn shorten(value: &str, width: usize) -> String {
    let length = value.chars().count();
    if length <= width {
        return value.to_owned();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let left = (width - 1) / 2;
    let right = width - left - 1;
    format!(
        "{}…{}",
        value.chars().take(left).collect::<String>(),
        value.chars().skip(length - right).collect::<String>()
    )
}

fn diff_control_title(file: Option<&LoadedDocument>) -> &'static str {
    match file {
        Some(file) if file.document.diff.is_file_view() => FILE_CONTROL_TITLE,
        Some(file) if file.document.diff.can_show_file() => DIFF_CONTROLS_TITLE,
        _ => BASIC_DIFF_CONTROLS_TITLE,
    }
}

fn diff_controls_are_visible(width: u16, file: Option<&LoadedDocument>) -> bool {
    if file.is_some_and(|file| file.temporary) {
        return false;
    }
    let minimum = if file.is_some_and(|file| file.document.diff.is_file_view()) {
        u16::try_from(FILE_CONTROL_TITLE.width() + 2).unwrap_or(u16::MAX)
    } else {
        MIN_DIFF_CONTROLS_WIDTH
    };
    width >= minimum
}

impl WrappedDiffRow {
    fn wrap_source(
        line: &Line<'static>,
        index: usize,
        width: u16,
        continuation_indent: usize,
        frame: Option<DiffFrame>,
        cursor_background: Option<Color>,
    ) -> Vec<Self> {
        let content_width = if frame.is_some() {
            width.saturating_sub(1)
        } else {
            width
        };
        wrap_line(line, content_width, continuation_indent)
            .into_iter()
            .map(|(mut line, source_display_offset)| {
                if let Some(background) = cursor_background {
                    fill_line_background(&mut line, width, background);
                }
                let border_cells =
                    frame.map_or_else(Vec::new, |frame| frame.enclose_line(&mut line));
                Self::source(line, border_cells, index, source_display_offset)
            })
            .collect()
    }

    fn source(
        line: Line<'static>,
        guide_border_cells: Vec<GuideBorderCell>,
        source_row: usize,
        source_display_offset: usize,
    ) -> Self {
        Self {
            line,
            guide_border_cells,
            source_row,
            source_display_offset,
            comment: None,
            editor: false,
            reply: None,
            guide_line: None,
            is_source_row: true,
        }
    }

    fn from_comment(row: crate::comment_layout::CommentRow) -> Self {
        Self {
            line: row.rendered.line,
            comment: row.target,
            reply: row.reply,
            editor: row.editor,
            guide_line: None,
            guide_border_cells: row.rendered.border_cells,
            source_row: row.rendered.source_row,
            source_display_offset: 0,
            is_source_row: false,
        }
    }
}
