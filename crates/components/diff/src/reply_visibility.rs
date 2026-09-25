use std::collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::ops::Range;

use ratatui::{
    buffer::{Buffer, Cell},
    layout::Rect,
    text::Line,
};
use review_threads::{MessageId, ThreadCommand};
use review_types::ReviewUnit;

use crate::{Action, ClippedViewport, DiffComponent};

#[derive(Default)]
pub(super) struct ReplyVisibility {
    unit: Option<ReviewUnit>,
    pending: BTreeMap<MessageId, ReplyFrame>,
    seen: BTreeMap<MessageId, SeenReply>,
}

#[derive(Default)]
struct ReplyFrame {
    layout: DefaultHasher,
    total: usize,
    visible: Vec<VisibleRow>,
}

struct VisibleRow {
    index: usize,
    area: Rect,
    cells: Vec<Cell>,
}

#[derive(Default)]
struct SeenReply {
    layout: u64,
    rows: BTreeSet<usize>,
}

impl ReplyVisibility {
    pub(super) fn project(&mut self, viewport: ClippedViewport) {
        for reply in self.pending.values_mut() {
            reply.visible.retain_mut(|row| {
                let Some(area) = viewport.project_row(row.area) else {
                    return false;
                };
                row.area = area;
                true
            });
        }
    }
    pub(super) fn observe<'a>(
        &mut self,
        rows: impl Iterator<Item = (Option<&'a MessageId>, &'a Line<'static>)>,
        visible: Range<usize>,
        area: Rect,
    ) {
        for (index, (id, line)) in rows.enumerate() {
            let Some(id) = id else { continue };
            let reply = self.pending.entry(id.clone()).or_default();
            area.width.hash(&mut reply.layout);
            for span in &line.spans {
                span.content.hash(&mut reply.layout);
            }
            if visible.contains(&index) && line.width() <= usize::from(area.width) {
                reply.visible.push(VisibleRow {
                    index: reply.total,
                    area: Rect::new(
                        area.x,
                        area.y + u16::try_from(index - visible.start).unwrap_or(u16::MAX),
                        area.width,
                        1,
                    ),
                    cells: Vec::new(),
                });
            }
            reply.total += 1;
        }
    }

    fn capture(&mut self, buffer: &Buffer) {
        for reply in self.pending.values_mut() {
            for row in &mut reply.visible {
                row.cells = row
                    .area
                    .positions()
                    .filter_map(|position| buffer.cell(position).cloned())
                    .collect();
            }
        }
    }

    fn finish(&mut self, buffer: &Buffer) {
        for reply in self.pending.values_mut() {
            reply.visible.retain(|row| {
                row.cells.len() == usize::from(row.area.width)
                    && row
                        .area
                        .positions()
                        .zip(&row.cells)
                        .all(|(position, expected)| buffer.cell(position) == Some(expected))
            });
        }
    }

    fn completed(&mut self) -> Vec<MessageId> {
        let mut complete = Vec::new();
        for (id, reply) in std::mem::take(&mut self.pending) {
            let layout = reply.layout.finish();
            let seen = self.seen.entry(id.clone()).or_default();
            if seen.layout != layout {
                *seen = SeenReply {
                    layout,
                    ..SeenReply::default()
                };
            }
            seen.rows
                .extend(reply.visible.into_iter().map(|row| row.index));
            if seen.rows.len() == reply.total && reply.total > 0 {
                complete.push(id);
            }
        }
        complete
    }
}

impl DiffComponent {
    /// Start observations for a fresh frame, including frames with no detail pane.
    pub fn begin_reply_frame(&self) {
        for viewer in self.embedded.saved.values() {
            viewer.begin_reply_frame();
        }
        let unit = self
            .comments
            .book
            .as_ref()
            .map(|book| book.review_unit.clone());
        let mut visibility = self.reply_visibility.borrow_mut();
        if visibility.unit != unit {
            *visibility = ReplyVisibility {
                unit,
                ..ReplyVisibility::default()
            };
        }
        visibility.pending.clear();
    }

    /// Capture rendered reply rows before application overlays are drawn.
    pub fn capture_reply_frame(&self, buffer: &Buffer) {
        for viewer in self.embedded.saved.values() {
            viewer.capture_reply_frame(buffer);
        }
        self.reply_visibility.borrow_mut().capture(buffer);
    }

    /// Exclude rows covered by a popup, menu, notification, or other overlay.
    pub fn finish_reply_frame(&self, buffer: &Buffer) {
        for viewer in self.embedded.saved.values() {
            viewer.finish_reply_frame(buffer);
        }
        self.reply_visibility.borrow_mut().finish(buffer);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn replies_displayed(&mut self, _: &ui_events::FrameRendered) -> Vec<Action> {
        self.displayed_reply_actions()
    }

    fn displayed_reply_actions(&mut self) -> Vec<Action> {
        let mut actions: Vec<_> = self
            .embedded
            .saved
            .values_mut()
            .flat_map(|viewer| viewer.displayed_reply_actions())
            .collect();
        let Some(book) = &self.comments.book else {
            return actions;
        };
        let visibility = self.reply_visibility.get_mut();
        if visibility.unit.as_ref() != Some(&book.review_unit) {
            return actions;
        }
        let messages = visibility
            .completed()
            .into_iter()
            .filter(|id| {
                book.thread_for_message(id)
                    .is_some_and(|thread| thread.has_unread_reply(id))
            })
            .collect::<Vec<_>>();
        if messages.is_empty() {
            return actions;
        }
        actions.push(Action::Thread(ThreadCommand::MarkRepliesRead {
            review_unit: book.review_unit.clone(),
            messages,
        }));
        actions
    }
}
