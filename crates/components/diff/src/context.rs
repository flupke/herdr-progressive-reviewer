use ui_events::PresentationLocation;

use diff_position::ScreenAnchor;

use crate::document::DiffDocument;
use crate::render::DiffViewport;
use crate::{DiffPresentation, PresentedRow, SourceViewer};

/// The cursor's place in the document and on screen, kept while context folds.
struct CursorAnchor {
    location: PresentationLocation,
    column: usize,
    screen: ScreenAnchor,
}

impl CursorAnchor {
    fn new(document: &DiffDocument, viewport: &DiffViewport) -> Option<Self> {
        let position = document.position();
        Some(Self {
            location: document.diff.presentation_location(position.cursor())?,
            column: position.column(),
            screen: position.screen_anchor(&document.laid_out(viewport)),
        })
    }

    fn restore_cursor(&self, document: &mut DiffDocument) {
        if let Some(row) = document.diff.row_at_location(self.location) {
            document.move_cursor(row);
            document.set_column(self.column);
        }
    }
}

impl SourceViewer {
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
        let Some(viewport) = self.displayed_viewport() else {
            return false;
        };
        let document = self.displayed_document_mut().expect("the document exists");
        let anchor = CursorAnchor::new(&document.document, &viewport);
        if !change(&mut document.document.diff) {
            return false;
        }
        if let Some(anchor) = anchor {
            anchor.restore_cursor(&mut document.document);
            let viewport = self.displayed_viewport().expect("the document exists");
            let document = self.displayed_document_mut().expect("the document exists");
            let (position, rows) = document.document.on_screen(&viewport);
            position.return_to(anchor.screen, &rows);
        }
        true
    }
}
