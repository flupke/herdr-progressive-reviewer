//! Accepting and reopening one hunk from its corner control or the cursor.

use ui_actions::{Action, RepositoryAction};
use ui_events::PointerInput;

use crate::SourceViewer;

impl SourceViewer {
    /// Accept the open hunk at `row` of the selected document, or reopen the
    /// reviewed one. One mark per file at a time: the next waits for the
    /// reloaded diff, because accepting a hunk moves the lines of the others.
    /// A file that no longer needs review has no hunk controls.
    pub(super) fn toggle_hunk_review(&mut self, row: usize) -> Vec<Action> {
        if !self.role.publishes_review() {
            return Vec::new();
        }
        let Some(review_checkpoint) = self.review_checkpoint.clone() else {
            return Vec::new();
        };
        let Some((path, mark)) = self
            .selected_document()
            .filter(|document| {
                self.reviewable_files.contains(&document.path)
                    && !self.pending_hunk_marks.contains(&document.path)
            })
            .and_then(|document| {
                let mark = document.document.diff.hunk_mark(row)?;
                Some((document.path.clone(), mark))
            })
        else {
            return Vec::new();
        };
        self.pending_hunk_marks.insert(path.clone());
        vec![Action::Repository(RepositoryAction::SetHunkReviewed {
            review_checkpoint,
            path,
            mark,
        })]
    }

    /// The document row whose hunk control the pointer is on.
    pub(super) fn hunk_control_at(&self, input: &PointerInput) -> Option<usize> {
        let position = input.position?;
        let screen_row = usize::from(position.component_row.checked_sub(1)?);
        let pane_column = usize::from(position.component_column.saturating_sub(1));
        self.rendered_pointer_viewport
            .borrow()
            .as_ref()?
            .badge_at(screen_row, pane_column)
    }

    /// A saved mark releases the next one, unless its document still has to
    /// reload the diff the next mark would act on.
    pub(super) fn hunk_mark_saved(&mut self, path: &str, reloading: bool) {
        if !reloading {
            self.pending_hunk_marks.remove(path);
        }
    }

    /// A finished or failed reload of a marked document takes the next mark.
    pub(super) fn hunk_marks_reloaded(&mut self, path: &str) {
        self.pending_hunk_marks.remove(path);
    }
}
