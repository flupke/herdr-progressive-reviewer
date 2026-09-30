//! Explore's lazily opened evidence viewers, and how one viewer draws inside
//! an Explore conversation.
use crate::explore::EvidenceShown;
use crate::{ClippedViewport, Role, Services, SourceViewer};
use ratatui::buffer::Buffer;
use review_explore::{Comparison, EvidencePosition};
use std::collections::BTreeMap;
use std::sync::Arc;
use ui_actions::Action;
use ui_events::{
    EvidenceView, ExploreEvidence, ExploreEvidenceInput, ExploreViewports, RepositoryFilesChanged,
};
use ui_theme::Palette;

/// Every viewer Explore opened, and the one in front.
#[derive(Default)]
pub(super) struct ExploreViewers {
    /// The pane shows Explore instead of the Files diff.
    shown: bool,
    /// The viewer Explore shows before any evidence opens. It is `None`
    /// until Explore first opens and while an evidence viewer is in front.
    base: Option<SourceViewer>,
    evidence: BTreeMap<EvidenceView, SourceViewer>,
    active: Option<EvidenceView>,
    /// Recovered positions that wait for their viewer's first viewport.
    restored: Vec<EvidencePosition>,
    /// The repository change the Files viewer applies once Explore closes.
    latest: Option<RepositoryFilesChanged>,
}

impl ExploreViewers {
    pub(super) fn is_shown(&self) -> bool {
        self.shown
    }

    /// Show Explore, opening its base viewer the first time, or hide it.
    /// Returns the repository change deferred while Explore was shown.
    pub(super) fn show(
        &mut self,
        shown: bool,
        services: &Services,
    ) -> Option<RepositoryFilesChanged> {
        self.shown = shown;
        if shown {
            self.open(services);
            return None;
        }
        self.latest.take()
    }

    /// Keep the latest repository change for the Files viewer until Explore closes.
    pub(super) fn defer(&mut self, event: &RepositoryFilesChanged) {
        self.latest = Some(event.clone());
    }

    /// The viewer in front: the active evidence viewer, else the base one.
    pub(super) fn current(&self) -> Option<&SourceViewer> {
        match self.active {
            Some(id) => self.evidence.get(&id),
            None => self.base.as_ref(),
        }
    }

    pub(super) fn current_mut(&mut self) -> Option<&mut SourceViewer> {
        match self.active {
            Some(id) => self.evidence.get_mut(&id),
            None => self.base.as_mut(),
        }
    }

    /// Every viewer.
    pub(super) fn viewers(&self) -> impl Iterator<Item = &SourceViewer> {
        self.base.iter().chain(self.evidence.values())
    }

    /// Every viewer, each with whether it is in front.
    pub(super) fn viewers_mut(&mut self) -> impl Iterator<Item = (bool, &mut SourceViewer)> {
        let active = self.active;
        self.base.iter_mut().map(|viewer| (true, viewer)).chain(
            self.evidence
                .iter_mut()
                .map(move |(id, viewer)| (active == Some(*id), viewer)),
        )
    }

    /// Open the base viewer the first time Explore is shown.
    fn open(&mut self, services: &Services) {
        if self.current().is_none() {
            self.base = Some(SourceViewer::new(
                services,
                Role::Evidence,
                ui_events::ReviewableFiles::default(),
            ));
        }
    }

    /// Show `comparison` in the viewer in front, closing every other viewer.
    /// Returns whether a viewer took it.
    pub(super) fn accept(&mut self, comparison: Arc<Comparison>) -> bool {
        let viewer = match self.active.take() {
            Some(id) => self.evidence.remove(&id),
            None => self.base.take(),
        };
        let Some(mut viewer) = viewer else {
            return false;
        };
        self.evidence.clear();
        self.restored.clear();
        viewer.install_comparison(comparison);
        self.base = Some(viewer);
        true
    }

    /// Bring the viewer of `event`'s evidence to the front, opening it on
    /// first use with the threads the viewer in front already loaded.
    pub(super) fn activate(
        &mut self,
        services: &Services,
        event: &ExploreEvidence,
    ) -> EvidenceShown {
        if self.active == Some(event.view) {
            return EvidenceShown {
                opened: false,
                switched: false,
            };
        }
        let opened = !self.evidence.contains_key(&event.view);
        if opened {
            let mut viewer = SourceViewer::new(
                services,
                Role::Evidence,
                ui_events::ReviewableFiles::default(),
            );
            viewer.install_comparison(event.comparison.clone());
            if let Some(book) = self
                .current()
                .and_then(|current| current.comments.book())
                .filter(|book| book.review_unit == event.comparison.checkpoint.review_unit)
            {
                viewer.comments.inherit_book(book);
            }
            viewer.refresh_comment_documents();
            self.evidence.insert(event.view, viewer);
        }
        self.base = None;
        self.active = Some(event.view);
        let viewer = self.current_mut().expect("the activated viewer exists");
        viewer.source_session = Some(format!("explore:{}", uuid::Uuid::new_v4()));
        services.events.publish(ui_events::SourceSessionChanged {
            snapshot_id: viewer.source_session.clone(),
        });
        EvidenceShown {
            opened,
            switched: true,
        }
    }

    /// The viewer showing `id`, while it is open.
    pub(super) fn view(&self, id: EvidenceView) -> Option<&SourceViewer> {
        self.evidence.get(&id)
    }

    pub(super) fn pointer(&mut self, event: &ExploreEvidenceInput) -> Vec<Action> {
        if self.active != Some(event.view) {
            return Vec::new();
        }
        self.current_mut()
            .map_or_else(Vec::new, |viewer| viewer.pointer_input(event.input))
    }

    pub(super) fn viewports(&mut self, event: &ExploreViewports) {
        for (id, viewport) in &event.0 {
            let restored = (self.active == Some(*id))
                .then(|| self.take_restored(*id))
                .flatten();
            if let Some(viewer) = self.evidence.get_mut(id) {
                if let Some(position) = restored {
                    viewer.restore_evidence_position(&position);
                }
                viewer.evidence_viewport(*viewport);
            }
        }
    }

    fn take_restored(&mut self, id: EvidenceView) -> Option<EvidencePosition> {
        let EvidenceView::Question { turn, reference } = id else {
            return None;
        };
        let index = self
            .restored
            .iter()
            .position(|position| position.turn == turn && position.reference == reference)?;
        Some(self.restored.remove(index))
    }

    pub(super) fn restore_positions(&mut self, positions: &[EvidencePosition]) {
        self.restored = positions.to_vec();
    }

    /// The position of every question's evidence viewer, for Explore to save.
    pub(super) fn positions(&self) -> Vec<EvidencePosition> {
        if self.current().is_none() {
            return self.restored.clone();
        }
        let mut result = self.restored.clone();
        for (id, viewer) in &self.evidence {
            let EvidenceView::Question { turn, reference } = *id else {
                continue;
            };
            if result
                .iter()
                .any(|saved| saved.turn == turn && saved.reference == reference)
            {
                // The first viewport event has not applied this recovered position yet.
                continue;
            }
            if let Some(file) = viewer.displayed_document() {
                result.push(EvidencePosition {
                    turn,
                    reference,
                    scroll: file.document.position().scroll(),
                    cursor: file.document.position().cursor(),
                    column: file.document.position().column(),
                });
            }
        }
        result.sort_by_key(|position| (position.turn, position.reference));
        result
    }
}

impl SourceViewer {
    fn restore_evidence_position(&mut self, saved: &EvidencePosition) {
        if let Some(file) = self.displayed_document_mut() {
            file.document.restore(diff_position::Position::new(
                saved.cursor,
                saved.column,
                saved.scroll,
            ));
        }
        self.evidence.fit_pending = false;
    }

    fn evidence_viewport(&mut self, viewport: ui_events::DiffViewportChanged) {
        self.viewport_changed(&viewport);
        if !std::mem::take(&mut self.evidence.fit_pending) {
            return;
        }
        let Some(file) = self.displayed_document() else {
            return;
        };
        let Some(evidence) = self.evidence.evidence.get(self.evidence.selected) else {
            return;
        };
        let Some(source) = self
            .evidence
            .comparison
            .as_ref()
            .and_then(|comparison| comparison.source(&evidence.location))
        else {
            return;
        };
        let rows = self.renderer(self.palette, false).evidence_rows(
            file,
            viewport.width,
            source.side,
            evidence.location.lines.as_ref(),
        );
        self.displayed_document_mut()
            .expect("evidence document")
            .document
            .reveal_evidence(rows.range, rows.total, usize::from(viewport.height));
    }

    /// Displayed source may differ from the question's immutable evidence after navigation.
    pub fn evidence_path(&self) -> Option<&str> {
        self.displayed_document()
            .map(|file| file.display_path.as_str())
    }

    pub fn evidence_limitation(&self) -> Option<&str> {
        self.evidence.limitation.as_deref()
    }

    /// Render a window through the native engine, then clip it to the conversation viewport.
    pub fn render_embedded(
        &self,
        viewport: ClippedViewport,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
    ) {
        let area = viewport.area();
        let mut window = Buffer::empty(area);
        self.render(area, &mut window, palette, focused)
            .render(&mut window);
        viewport.draw(&window, buffer);
        let mut replies = self.reply_visibility.borrow_mut();
        replies.project(viewport);
        replies.capture(buffer);
    }
}
