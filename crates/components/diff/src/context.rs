use ui_events::PresentationLocation;

use crate::document::DiffDocument;
use crate::{DiffComponent, DiffPresentation, PresentedRow};

struct CursorAnchor {
    location: PresentationLocation,
    column: usize,
    visual_row: usize,
    scroll: usize,
}

impl CursorAnchor {
    fn new(document: &DiffDocument, visual_row: usize) -> Option<Self> {
        Some(Self {
            location: document.diff.presentation_location(document.cursor)?,
            column: document.column,
            visual_row,
            scroll: document.scroll,
        })
    }

    fn restore_cursor(&self, document: &mut DiffDocument) {
        if let Some(row) = document.diff.row_at_location(self.location) {
            document.cursor = row;
            document.column = self.column;
        }
    }

    fn scroll(&self, visual_row: usize) -> usize {
        self.scroll
            .saturating_add(visual_row)
            .saturating_sub(self.visual_row)
    }
}

impl DiffComponent {
    pub(super) fn expand_context(&mut self, row: usize) -> bool {
        let is_gap = self.displayed_document().is_some_and(|document| {
            matches!(
                document.document.diff.rows.get(row),
                Some(PresentedRow::Gap { .. })
            )
        });
        is_gap && self.change_context(|diff| diff.expand(row))
    }

    pub(super) fn change_context(
        &mut self,
        change: impl FnOnce(&mut DiffPresentation) -> bool,
    ) -> bool {
        let Some(document) = self.displayed_document() else {
            return false;
        };
        let viewport = self.displayed_viewport().expect("the document exists");
        let anchor = CursorAnchor::new(&document.document, viewport.cursor_visual_row(document));
        let document = self.displayed_document_mut().expect("the document exists");
        if !change(&mut document.document.diff) {
            return false;
        }
        if let Some(anchor) = anchor {
            anchor.restore_cursor(&mut document.document);
            let viewport = self.displayed_viewport().expect("the document exists");
            let document = self.displayed_document_mut().expect("the document exists");
            document.document.scroll = anchor.scroll(viewport.cursor_visual_row(document));
        }
        true
    }
}
