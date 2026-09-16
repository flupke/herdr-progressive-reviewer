//! Resolved inline threads retain a compact summary.

use ratatui::{style::Style, text::Line};
use review_threads::{Resolution, ReviewThread};

use super::{CommentRow, thread::ThreadLayout};
use crate::comments::Comments;

impl Comments {
    pub(super) fn inline_thread_rows(
        &self,
        thread: &ReviewThread,
        layout: ThreadLayout,
    ) -> Vec<CommentRow> {
        if thread.resolution == Resolution::Resolved {
            let mut rows = layout.collapsed(thread);
            layout.controls(&mut rows, Some(thread), false);
            rows
        } else {
            self.thread_rows(thread, layout)
        }
    }
}

impl ThreadLayout {
    fn collapsed(self, thread: &ReviewThread) -> Vec<CommentRow> {
        let question = thread
            .messages
            .first()
            .and_then(|message| message.text.lines().next())
            .unwrap_or_default();
        self.frame
            .wrapped_content(
                &Line::styled(
                    format!("Resolved · {question}"),
                    Style::default().fg(self.palette.dim),
                ),
                self.source_row,
            )
            .into_iter()
            .take(1)
            .map(|row| CommentRow::new(row, None))
            .collect()
    }
}
