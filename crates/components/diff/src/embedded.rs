//! Independent, lazily opened native viewers sharing the application's I/O services.
use crate::{ClippedViewport, DiffComponent};
use ratatui::buffer::Buffer;
use std::collections::BTreeMap;
use ui_actions::Action;
use ui_events::{EvidenceView, ExploreEvidence, ExploreEvidenceInput, ExploreViewports};
use ui_theme::Palette;

#[derive(Default)]
pub(super) struct EmbeddedViews {
    pub(super) active: Option<EvidenceView>,
    pub(super) restored: Vec<review_explore::EvidencePosition>,
    pub(super) saved: BTreeMap<EvidenceView, Box<DiffComponent>>,
}

impl DiffComponent {
    pub(super) fn retained_viewers_mut(&mut self) -> impl Iterator<Item = &mut Self> {
        self.embedded
            .saved
            .values_mut()
            .chain(self.explore.parked.iter_mut())
            .map(AsMut::as_mut)
    }

    pub(super) fn retain_search_results(&mut self, results: &text_search::Results) {
        self.finish_search(results, false);
        for viewer in self.retained_viewers_mut() {
            viewer.retain_search_results(results);
        }
    }

    pub(super) fn retained_evidence_actions(&mut self, switched: bool) -> Vec<Action> {
        let mut actions = self.request_visible_highlights();
        if switched {
            actions.extend(self.resume_evidence_search());
        }
        actions
    }

    pub(super) fn resume_evidence_search(&mut self) -> Option<Action> {
        self.publish_search_status();
        // The shared worker may have replaced this request while another window was active.
        if let Some(request) = self
            .search
            .as_ref()
            .and_then(|search| search.pending.clone())
        {
            Some(Action::Search(Some(request)))
        } else {
            self.refresh_search_matches()
        }
    }

    /// Borrow the real viewer retained for an opened evidence block.
    pub fn evidence_view(&self, id: EvidenceView) -> Option<&Self> {
        if self.embedded.active == Some(id) {
            Some(self)
        } else {
            self.embedded.saved.get(&id).map(AsRef::as_ref)
        }
    }

    pub(super) fn activate_evidence_view(&mut self, event: &ExploreEvidence) -> bool {
        if self.embedded.active == Some(event.view) {
            return false;
        }
        let mut views = std::mem::take(&mut self.embedded);
        let existing = views.saved.contains_key(&event.view);
        let mut next = views.saved.remove(&event.view).unwrap_or_else(|| {
            let mut next = Self::new(
                self.events.clone(),
                ui_events::ReviewableFiles::default(),
                self.highlighter.clone(),
                self.repository_root.clone(),
                self.palette,
            );
            next.next_search_id = self.next_search_id.clone();
            next.explore.active = true;
            next.install_comparison(event.comparison.clone());
            next.comments.share_draft_cancellations(&self.comments);
            if let Some(book) = self
                .comments
                .book
                .as_ref()
                .filter(|book| book.review_unit == event.comparison.checkpoint.review_unit)
            {
                next.comments.inherit_book(book);
            }
            next.refresh_comment_documents();
            Box::new(next)
        });
        let parked = self.explore.parked.take();
        let latest = self.explore.latest.take();
        std::mem::swap(self, &mut next);
        self.explore.parked = parked;
        self.explore.latest = latest;
        if let Some(previous) = views.active.replace(event.view) {
            views.saved.insert(previous, next);
        }
        self.embedded = views;
        self.source_session = Some(format!("explore:{}", uuid::Uuid::new_v4()));
        self.events.publish(ui_events::SourceSessionChanged {
            snapshot_id: self.source_session.clone(),
        });
        !existing
    }

    pub(super) fn embedded_pointer(&mut self, event: &ExploreEvidenceInput) -> Vec<Action> {
        if self.embedded.active == Some(event.view) {
            self.pointer_input(event.input)
        } else {
            Vec::new()
        }
    }

    pub(super) fn embedded_viewports(&mut self, event: &ExploreViewports) {
        for (id, viewport) in &event.0 {
            if self.embedded.active == Some(*id) {
                self.evidence_viewport(*viewport);
            } else if let Some(viewer) = self.embedded.saved.get_mut(id) {
                viewer.evidence_viewport(*viewport);
            }
        }
    }

    fn evidence_viewport(&mut self, viewport: ui_events::DiffViewportChanged) {
        if let Some(active) = self.embedded.active
            && let Some(index) = self.embedded.restored.iter().position(|position| {
                position.turn == active.turn && position.reference == active.reference
            })
        {
            let saved = self.embedded.restored.remove(index);
            if let Some(file) = self.displayed_document_mut() {
                let maximum = file.document.diff.len().saturating_sub(1);
                file.document.scroll = saved.scroll.min(maximum);
                file.document.cursor = saved.cursor.min(maximum);
                file.document.column = saved.column;
            }
            self.explore.fit_pending = false;
        }
        self.viewport_changed(&viewport);
        if !std::mem::take(&mut self.explore.fit_pending) {
            return;
        }
        let Some(file) = self.displayed_document() else {
            return;
        };
        let Some(evidence) = self.explore.evidence.get(self.explore.selected) else {
            return;
        };
        let Some(source) = self
            .explore
            .comparison
            .as_ref()
            .and_then(|comparison| comparison.source(&evidence.location))
        else {
            return;
        };
        let scroll = self.renderer(self.palette, None, false).evidence_scroll(
            file,
            viewport.width,
            viewport.height,
            source.side,
            evidence.location.lines.as_ref(),
        );
        self.displayed_document_mut()
            .expect("evidence document")
            .document
            .scroll = scroll;
    }

    /// Size the primary range using the native renderer's wrapped rows, including outlines.
    pub fn fitted_evidence_height(&self, width: u16, maximum: u16) -> u16 {
        let minimum = maximum.min(5);
        let Some(file) = self.selected_document() else {
            return minimum;
        };
        let Some(evidence) = self.explore.evidence.get(self.explore.selected) else {
            return minimum;
        };
        let Some(source) = self
            .explore
            .comparison
            .as_ref()
            .and_then(|comparison| comparison.source(&evidence.location))
        else {
            return minimum;
        };
        self.measure_evidence_height(
            file,
            width.saturating_sub(2),
            source.side,
            evidence.location.lines.as_ref(),
        )
        .clamp(minimum, maximum.max(minimum))
    }

    /// Displayed source may differ from the question's immutable evidence after navigation.
    pub fn evidence_path(&self) -> Option<&str> {
        self.displayed_document()
            .map(|file| file.display_path.as_str())
    }

    pub fn evidence_limitation(&self) -> Option<&str> {
        self.explore.limitation.as_deref()
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
        self.render(area, &mut window, palette, focused, None)
            .render(&mut window);
        viewport.draw(&window, buffer);
        self.reply_visibility.borrow_mut().project(viewport);
    }
}

impl DiffComponent {
    pub fn explore_positions(&self) -> Vec<review_explore::EvidencePosition> {
        if !self.explore.active {
            return self.explore.parked.as_ref().map_or_else(
                || self.explore.positions.clone(),
                |viewer| viewer.explore_positions(),
            );
        }
        let mut result = self.embedded.restored.clone();
        for (id, viewer) in self
            .embedded
            .saved
            .iter()
            .map(|(id, view)| (*id, view.as_ref()))
            .chain(self.embedded.active.map(|id| (id, self)))
        {
            if result
                .iter()
                .any(|saved| saved.turn == id.turn && saved.reference == id.reference)
            {
                // The first viewport event has not applied this recovered position yet.
                continue;
            }
            if let Some(file) = viewer.displayed_document() {
                result.retain(|saved| saved.turn != id.turn || saved.reference != id.reference);
                result.push(review_explore::EvidencePosition {
                    turn: id.turn,
                    reference: id.reference,
                    scroll: file.document.scroll,
                    cursor: file.document.cursor,
                    column: file.document.column,
                });
            }
        }
        result.sort_by_key(|position| (position.turn, position.reference));
        result
    }

    pub(super) fn restore_explore_positions(
        &mut self,
        event: &ui_events::ExplorePositionsRestored,
    ) {
        if self.explore.active {
            self.embedded.restored.clone_from(&event.0);
        } else if let Some(parked) = &mut self.explore.parked {
            parked.embedded.restored.clone_from(&event.0);
        } else {
            self.explore.positions.clone_from(&event.0);
        }
    }
}
