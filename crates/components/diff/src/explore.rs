use crate::{DiffComponent, LoadedDocument, presentation::DiffPresentation};
use review_explore::{Comparison, EvidenceRef, SourceSide};
use review_repository::diff::parse_file_diff;
use std::sync::Arc;
use ui_actions::Action;
use ui_events::{
    ExploreComparisonAccepted, ExploreEvidence, RepositoryFilesChanged, ReviewNavigation,
    ReviewNavigationChanged, SourceSessionChanged,
};

#[derive(Default)]
pub(super) struct ExploreView {
    pub(super) active: bool,
    pub(super) parked: Option<Box<DiffComponent>>,
    pub(super) comparison: Option<Arc<Comparison>>,
    pub(super) latest: Option<RepositoryFilesChanged>,
    pub(super) evidence: Vec<EvidenceRef>,
    pub(super) primary: usize,
    pub(super) selected: usize,
    pub(super) limitation: Option<String>,
    pub(super) fit_pending: bool,
    pub(super) positions: Vec<review_explore::EvidencePosition>,
}

impl ExploreView {
    pub(super) fn identify_source(
        &self,
        document: &mut LoadedDocument,
        path: &std::path::Path,
        root: &std::path::Path,
    ) {
        if self.active
            && let Some(source) = self.source(path, root)
        {
            document.path.clone_from(&source.display_path);
            document.display_path.clone_from(&source.display_path);
        }
    }

    fn source(
        &self,
        path: &std::path::Path,
        root: &std::path::Path,
    ) -> Option<review_explore::Source> {
        self.comparison
            .as_ref()?
            .working_source_at(path.strip_prefix(root).ok()?)
    }
    pub(super) fn ranges(
        &self,
        file: &LoadedDocument,
    ) -> Vec<(SourceSide, review_guide::GuideLineRange)> {
        let Some(comparison) = &self.comparison else {
            return Vec::new();
        };
        self.evidence
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                *index == self.selected || (self.selected < self.primary && *index < self.primary)
            })
            .filter_map(|(_, evidence)| {
                let source = comparison.source(&evidence.location)?;
                let matches = match source.side {
                    SourceSide::Old => file.old_path.as_deref(),
                    SourceSide::New => file.new_path.as_deref(),
                }
                .is_some_and(|path| path == source.display_path)
                    || file.path == source.display_path;
                matches
                    .then(|| {
                        evidence
                            .location
                            .lines
                            .clone()
                            .map(|range| (source.side, range))
                    })
                    .flatten()
            })
            .collect()
    }
}

impl DiffComponent {
    fn comparison_document(&self, comparison: &Comparison, index: usize) -> LoadedDocument {
        let file = &comparison.files[index];
        let Some(context) = comparison.context.get(index) else {
            return LoadedDocument::from_summary(&ui_events::FileSummary::from_review_state(
                file,
                review_state::ReviewState::unreviewed(file.statistics, None),
            ));
        };
        let rows = parse_file_diff(&comparison.diffs[index], file);
        let loaded = Arc::new(ui_events::DiffContentLoaded {
            review_checkpoint: comparison.checkpoint.clone(),
            path: context.path.clone(),
            rows: rows.clone(),
            old_content: context.old_content.clone(),
            new_content: context.new_content.clone(),
        });
        self.comparison_content_document(file, loaded)
    }

    pub(super) fn comparison_content_document(
        &self,
        file: &review_repository::repository::ChangedFile,
        loaded: Arc<ui_events::DiffContentLoaded>,
    ) -> LoadedDocument {
        let summary = ui_events::FileSummary::from_review_state(
            file,
            review_state::ReviewState::unreviewed(file.statistics, None),
        );
        let mut document = LoadedDocument::from_summary(&summary);
        document.replace_diff(DiffPresentation::new(self.highlighter.plain(
            loaded.rows.clone(),
            loaded.old_content.as_deref(),
            loaded.new_content.as_deref(),
        )));
        document
            .document
            .prepare_highlighting(ui_events::HighlightRequest::Diff(loaded.clone()));
        document.content = Some(loaded);
        document
    }

    fn select_comparison_document(
        &mut self,
        comparison: &Comparison,
        index: usize,
    ) -> eyre::Result<()> {
        let path = comparison.files[index].review_path().display();
        if comparison.context.get(index).is_none() {
            let document = self.restored_comparison_document(comparison, index)?;
            self.documents.retain(|document| document.path != path);
            self.documents.push(document);
        }
        if !self.documents.iter().any(|document| document.path == path) {
            self.documents
                .push(self.comparison_document(comparison, index));
        }
        self.comments.restore_file_editor(&path);
        self.selected_path = Some(path);
        self.preview = None;
        Ok(())
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub(super) fn explore_navigation(&mut self, event: &ReviewNavigationChanged) -> Vec<Action> {
        let active = event.0 == ReviewNavigation::Explore;
        let switched = self.explore.active != active;
        let mut actions = Vec::new();
        if self.explore.active != active {
            let latest = self.explore.latest.take();
            let positions = std::mem::take(&mut self.explore.positions);
            let mut other = self.explore.parked.take().unwrap_or_else(|| {
                let mut viewer = Self::new(
                    self.events.clone(),
                    ui_events::ReviewableFiles::default(),
                    self.highlighter.clone(),
                    self.repository_root.clone(),
                    self.palette,
                );
                viewer.next_search_id = self.next_search_id.clone();
                viewer.comments.share_draft_cancellations(&self.comments);
                viewer.explore.active = active;
                Box::new(viewer)
            });
            std::mem::swap(self, &mut other);
            self.explore.parked = Some(other);
            if active {
                self.embedded.restored.extend(positions);
            }
            if !active && let Some(latest) = latest {
                actions.extend(self.repository_changed(&latest));
            }
            self.events.publish(SourceSessionChanged {
                snapshot_id: self.source_session.clone(),
            });
        }
        self.conversation_navigation(event);
        if switched && let Some(checkpoint) = &self.review_checkpoint {
            actions.push(Action::Thread(review_threads::ThreadCommand::Load(
                checkpoint.review_unit.clone(),
            )));
        }
        if switched && active {
            actions.extend(self.resume_evidence_search());
        }
        actions
    }

    pub(super) fn explore_comparison_accepted(
        &mut self,
        event: &ExploreComparisonAccepted,
    ) -> Vec<Action> {
        let comparison = &event.0;
        if !self.explore.active {
            if let Some(parked) = &mut self.explore.parked {
                parked.install_comparison(comparison.clone());
            }
            return Vec::new();
        }
        self.install_comparison(comparison.clone());
        self.events.publish(SourceSessionChanged {
            snapshot_id: self.source_session.clone(),
        });
        vec![Action::Thread(review_threads::ThreadCommand::Load(
            comparison.checkpoint.review_unit.clone(),
        ))]
    }

    pub(super) fn install_comparison(&mut self, comparison: Arc<Comparison>) {
        self.embedded = crate::embedded::EmbeddedViews::default();
        self.review_checkpoint = Some(comparison.checkpoint.clone());
        self.source_session = Some(format!("explore:{}", uuid::Uuid::new_v4()));
        self.documents = (0..comparison.files.len())
            .map(|index| self.comparison_document(&comparison, index))
            .collect();
        self.comments.paths = ui_events::FileSummary::thread_paths(
            &comparison
                .files
                .iter()
                .map(|file| {
                    ui_events::FileSummary::from_review_state(
                        file,
                        review_state::ReviewState::unreviewed(file.statistics, None),
                    )
                })
                .collect::<Vec<_>>(),
        );
        self.explore.comparison = Some(comparison);
        self.explore.evidence.clear();
        self.selected_path = None;
        self.preview = None;
        self.selection = None;
        self.search = None;
    }

    pub(super) fn explore_evidence(&mut self, event: &ExploreEvidence) -> Vec<Action> {
        if !self.explore.active {
            return Vec::new();
        }
        let switched = self.embedded.active != Some(event.view);
        let opened = self.activate_evidence_view(event);
        self.explore.comparison = Some(event.comparison.clone());
        self.explore.evidence.clone_from(&event.evidence);
        self.explore.primary = event.primary;
        if !opened && !event.reveal {
            return self.retained_evidence_actions(switched);
        }
        self.explore.selected = event.view.reference;
        self.explore.limitation = None;
        let Some(evidence) = event.evidence.get(event.view.reference) else {
            return Vec::new();
        };
        let Some(source) = event.comparison.source(&evidence.location) else {
            return Vec::new();
        };
        let content = match source.read_text(&self.repository_root) {
            Ok(content) => content,
            Err(error) => {
                self.explore.limitation = Some(format!(
                    "File-level evidence · non-text or unavailable source: {error}"
                ));
                return Vec::new();
            }
        };
        if evidence.location.lines.as_ref().is_some_and(|range| {
            range.first_line == 0
                || range.last_line < range.first_line
                || range.last_line as usize > content.lines().count()
        }) {
            self.explore.limitation =
                Some("The saved evidence range is unavailable in this source.".into());
            return Vec::new();
        }
        let file = event.comparison.files.iter().position(|file| match source.side {
            SourceSide::Old => file.old_path.as_ref(), SourceSide::New => file.new_path.as_ref(),
        } == Some(&source.path));
        if let Some(index) = file {
            return self.open_changed_evidence(
                &event.comparison,
                index,
                &source,
                &content,
                evidence,
            );
        }
        self.open_supporting_evidence(&source, &content, evidence.location.lines.as_ref())
    }

    fn open_changed_evidence(
        &mut self,
        comparison: &Comparison,
        index: usize,
        source: &review_explore::Source,
        content: &str,
        evidence: &EvidenceRef,
    ) -> Vec<Action> {
        if self.select_comparison_document(comparison, index).is_err() {
            return self.open_supporting_evidence(
                source,
                content,
                evidence.location.lines.as_ref(),
            );
        }
        if let Some(range) = &evidence.location.lines {
            let location = match source.side {
                SourceSide::Old => ui_events::PresentationLocation::OldLine(range.first_line - 1),
                SourceSide::New => ui_events::PresentationLocation::NewLine(range.first_line - 1),
            };
            self.reveal_evidence_range(source, content, range, location);
        }
        self.explore.fit_pending = true;
        self.request_visible_highlights()
    }

    pub(super) fn explore_source(
        &mut self,
        location: review_lsp::SourceLocation,
        mode: ui_events::SourceLoadMode,
    ) -> Vec<Action> {
        let source = self.explore.source(&location.path, &self.repository_root);
        let Some(source) = source else {
            self.explore_notice("Destination is outside the repository.");
            return Vec::new();
        };
        let content = match source.read_text(&self.repository_root) {
            Ok(text) => text.into_bytes(),
            Err(error) => {
                self.explore_notice(&format!("Source is non-text or unavailable: {error}"));
                return Vec::new();
            }
        };
        let path = source.display_path.clone();
        self.documents
            .retain(|document| !document.comments_only || document.path != path);
        if let Some(document) = self.documents.iter_mut().find(|document| {
            document.content.is_some()
                && !document.document.diff.is_base_file()
                && (document.new_path.as_deref() == Some(path.as_str()) || document.path == path)
        }) {
            if mode == ui_events::SourceLoadMode::Preview {
                let mut preview = document.clone();
                preview.document.reveal_location(&location);
                preview.disk_path = Some(location.path);
                self.preview = Some(preview);
                self.center_jump_target();
                return self.request_visible_highlights();
            }
            document.document.reveal_location(&location);
            document.disk_path = Some(location.path.clone());
            if mode.is_external() {
                self.comments.restore_file_editor(&document.path);
                self.selected_path = Some(document.path.clone());
                self.preview = None;
                self.center_jump_target();
                return self.request_visible_highlights();
            }
        }
        // Restored comparison entries contain metadata only. A source view must replace
        // that placeholder, otherwise path-based selection finds the empty entry first.
        self.documents
            .retain(|document| document.path != path || document.content.is_some());
        let event = ui_events::SourceContentLoaded {
            snapshot_id: self.source_session.clone().expect("Explore source view"),
            location,
            content,
            mode,
        };
        let actions = self.source_content_loaded(&event);
        let checkpoint = self
            .explore
            .comparison
            .as_ref()
            .expect("Explore comparison")
            .checkpoint
            .clone();
        if mode.is_external()
            && let Some(document) = self.selected_document_mut()
        {
            document.new_path = Some(path.clone());
            document.content = Some(Arc::new(ui_events::DiffContentLoaded {
                review_checkpoint: checkpoint,
                path: document.path.clone(),
                rows: Vec::new(),
                old_content: None,
                new_content: Some(event.content),
            }));
            self.comments.restore_file_editor(&path);
        }
        actions
    }

    pub(super) fn explore_lsp_ready(&self) -> bool {
        if self.selected_document().is_some_and(|file| {
            file.document
                .diff
                .source_position(file.document.cursor)
                .is_none()
        }) {
            self.explore_notice("LSP is available on new-side source lines only; old/deleted coordinates are not sent to the current document.");
            return false;
        }
        true
    }

    fn explore_notice(&self, text: &str) {
        self.events.publish(ui_events::ToastRequested {
            text: text.into(),
            kind: toasts::ToastKind::Info,
        });
    }
}
