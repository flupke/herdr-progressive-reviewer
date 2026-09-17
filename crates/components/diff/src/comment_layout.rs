//! Inline conversation frames share the guides' gutter, borders, and scrolling.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;

use guide_rendering::{DiffFrame, FrameRule, GuideRenderedRow};
use ratatui::style::Style;
use review_guide::FrozenHunk;
use review_threads::{MessageId, Resolution, ReviewThread};
use ui_events::PresentationLocation;
use ui_theme::Palette;

use crate::LoadedDocument;
use crate::comments::{CommentTarget, Comments};

mod controls;
mod inline;
mod thread;
use thread::ThreadLayout;

pub(super) struct CommentRow {
    pub(super) rendered: GuideRenderedRow,
    pub(super) target: Option<CommentTarget>,
    pub(super) editor: bool,
    pub(super) reply: Option<MessageId>,
}

pub(super) struct CommentLayout {
    include_code: bool,
    frame: DiffFrame,
    before: BTreeMap<usize, Vec<CommentRow>>,
    after: BTreeMap<usize, Vec<CommentRow>>,
    detached: Vec<CommentRow>,
    // Keep resolved boxes outside frames shared by overlapping open threads.
    collapsed: BTreeMap<usize, Vec<CommentRow>>,
    ranges: Vec<RangeInclusive<usize>>,
}

impl CommentLayout {
    fn new(frame: DiffFrame, include_code: bool) -> Self {
        Self {
            include_code,
            frame,
            before: BTreeMap::new(),
            after: BTreeMap::new(),
            detached: Vec::new(),
            collapsed: BTreeMap::new(),
            ranges: Vec::new(),
        }
    }

    pub(super) fn frame_at(&self, row: usize) -> Option<DiffFrame> {
        self.ranges
            .iter()
            .any(|range| range.contains(&row))
            .then_some(self.frame)
    }

    pub(super) fn take_before(&mut self, row: usize) -> Vec<CommentRow> {
        self.before.remove(&row).unwrap_or_default()
    }

    pub(super) fn take_after(&mut self, row: usize) -> Vec<CommentRow> {
        self.after
            .remove(&row)
            .into_iter()
            .flatten()
            .chain(self.collapsed.remove(&row).into_iter().flatten())
            .collect()
    }

    pub(super) fn into_remaining(self) -> impl Iterator<Item = CommentRow> {
        self.before
            .into_values()
            .chain(self.after.into_values())
            .chain(self.collapsed.into_values())
            .flatten()
            .chain(self.detached)
    }

    fn push_thread(
        &mut self,
        range: Option<RangeInclusive<usize>>,
        fallback: usize,
        content: Vec<CommentRow>,
        id: Option<&MessageId>,
    ) {
        let range = range.filter(|_| self.include_code);
        let source_row = range.as_ref().map_or(fallback, |range| *range.end());
        let detached = range.is_none();
        let (rule, rows) = if let Some(range) = range {
            self.ranges.push(range);
            (FrameRule::Middle, self.after.entry(source_row).or_default())
        } else {
            (FrameRule::Top(None), &mut self.detached)
        };
        rows.push(CommentRow::new(self.frame.rule(rule, source_row), id));
        rows.extend(content);
        if detached {
            rows.push(CommentRow::new(
                self.frame.rule(FrameRule::Bottom, source_row),
                id,
            ));
        }
    }

    fn push_collapsed(
        &mut self,
        range: Option<&RangeInclusive<usize>>,
        fallback: usize,
        content: Vec<CommentRow>,
    ) {
        let range = range.filter(|_| self.include_code);
        let source_row = range.map_or(fallback, |range| *range.end());
        let rows = if range.is_some() {
            self.collapsed.entry(source_row).or_default()
        } else {
            &mut self.detached
        };
        rows.push(CommentRow::new(
            self.frame.rule(FrameRule::Top(None), source_row),
            None,
        ));
        rows.extend(content);
        rows.push(CommentRow::new(
            self.frame.rule(FrameRule::Bottom, source_row),
            None,
        ));
    }

    fn enclose_ranges(&mut self) {
        let mut ranges = std::mem::take(&mut self.ranges);
        ranges.sort_by_key(|range| *range.start());
        for range in ranges {
            if let Some(previous) = self.ranges.last_mut()
                && range.start() <= previous.end()
            {
                *previous = *previous.start()..=(*previous.end()).max(*range.end());
            } else {
                self.ranges.push(range);
            }
        }
        for range in &self.ranges {
            self.before
                .entry(*range.start())
                .or_default()
                .push(CommentRow::new(
                    self.frame.rule(FrameRule::Top(None), *range.start()),
                    None,
                ));
            for (&row, content) in self.after.range_mut(range.clone()) {
                let rule = if row == *range.end() {
                    FrameRule::Bottom
                } else {
                    FrameRule::Middle
                };
                let id = content
                    .last()
                    .and_then(|row| row.target.as_ref())
                    .and_then(CommentTarget::id)
                    .cloned();
                content.push(CommentRow::new(self.frame.rule(rule, row), id.as_ref()));
            }
        }
    }
}

impl Comments {
    pub(super) fn conversation_rows(
        &self,
        thread: &ReviewThread,
        frame: DiffFrame,
        palette: Palette,
    ) -> Vec<CommentRow> {
        let layout = ThreadLayout {
            frame,
            source_row: 0,
            palette,
            outdated: false,
        };
        self.thread_rows(thread, layout)
    }

    fn thread_range(
        &self,
        thread: &ReviewThread,
        file: &LoadedDocument,
    ) -> Option<RangeInclusive<usize>> {
        self.mapped(&thread.id)
            .and_then(|range| anchor_rows(range, file))
    }

    pub(super) fn thread_row(&self, thread: &ReviewThread, file: &LoadedDocument) -> (usize, bool) {
        self.thread_range(thread, file).map_or(
            (file.document.diff.len().saturating_sub(1), true),
            |range| (*range.end(), false),
        )
    }

    pub(super) fn has_for(&self, file: &LoadedDocument) -> bool {
        self.inline_editor_visible_in(file)
            || self
                .threads()
                .any(|thread| self.matches_path(file, thread.path()))
    }

    pub(super) fn layout(
        &self,
        file: &LoadedDocument,
        width: u16,
        palette: Palette,
        include_code: bool,
    ) -> CommentLayout {
        let frame = DiffFrame::new(
            width,
            file.document.diff.line_number_width(),
            Style::default().fg(palette.focus),
        );
        let mut layout = CommentLayout::new(frame, include_code);
        if let Some(editing) = &self.editing
            && editing.draft.reply_to.is_none()
            && self.matches_path(file, editing.draft.path())
        {
            let range = file
                .content
                .as_ref()
                .and_then(|content| {
                    editing.draft.source.anchor.map_lines(
                        content.old_content.as_deref(),
                        content.new_content.as_deref(),
                    )
                })
                .and_then(|range| anchor_rows(&range, file));
            let source_row = range
                .as_ref()
                .map_or(file.document.diff.len().saturating_sub(1), |range| {
                    *range.end()
                });
            let mut rows = Vec::new();
            self.editor_rows(&mut rows, source_row, frame, palette);
            layout.push_thread(range, source_row, rows, None);
        }
        for thread in self
            .threads()
            .filter(|thread| self.matches_path(file, thread.path()))
        {
            let range = self.thread_range(thread, file);
            let source_row = range
                .as_ref()
                .map_or(file.document.diff.len().saturating_sub(1), |range| {
                    *range.end()
                });
            let rows = self.inline_thread_rows(
                thread,
                ThreadLayout {
                    frame,
                    source_row,
                    palette,
                    outdated: include_code && range.is_none(),
                },
            );
            if thread.resolution == Resolution::Resolved {
                layout.push_collapsed(range.as_ref(), source_row, rows);
            } else {
                layout.push_thread(
                    range,
                    source_row,
                    rows,
                    thread.messages.first().map(|comment| &comment.id),
                );
            }
        }
        layout.enclose_ranges();
        layout
    }
}

fn anchor_rows(
    range: &FrozenHunk,
    file: &LoadedDocument,
) -> Option<std::ops::RangeInclusive<usize>> {
    let mut matches = file
        .document
        .diff
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, _)| {
            let matches = match file.document.diff.presentation_location(index)? {
                PresentationLocation::Context { old_line, new_line } => {
                    range
                        .old
                        .as_ref()
                        .is_some_and(|range| range.contains(&old_line))
                        || range
                            .new
                            .as_ref()
                            .is_some_and(|range| range.contains(&new_line))
                }
                PresentationLocation::OldLine(line) => range
                    .old
                    .as_ref()
                    .is_some_and(|range| range.contains(&line)),
                PresentationLocation::NewLine(line) => range
                    .new
                    .as_ref()
                    .is_some_and(|range| range.contains(&line)),
                _ => false,
            };
            matches.then_some(index)
        });
    matches
        .next()
        .map(|first| first..=matches.next_back().unwrap_or(first))
        .or_else(|| {
            range
                .new
                .as_ref()
                .and_then(|range| range.end.checked_sub(1))
                .and_then(|line| {
                    file.document
                        .diff
                        .row_at_location(PresentationLocation::NewLine(line))
                        .map(|row| row..=row)
                })
        })
}

impl CommentRow {
    fn new(rendered: GuideRenderedRow, id: Option<&MessageId>) -> Self {
        Self {
            rendered,
            target: id.cloned().map(CommentTarget::Message),
            editor: false,
            reply: None,
        }
    }
}
