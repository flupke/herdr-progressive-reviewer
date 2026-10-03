use crate::{LoadedDocument, SourceViewer, presentation::DiffPresentation};
use review_explore::{CitedSource, Comparison, EvidenceRef, SourceSide, Uncitable};
use review_repository::diff::parse_file_diff;
use std::sync::Arc;
use ui_actions::Action;
use ui_events::ExploreEvidence;

/// The comparison and the evidence one Explore viewer shows.
#[derive(Default)]
pub(super) struct ShownEvidence {
    pub(super) comparison: Option<Arc<Comparison>>,
    pub(super) evidence: Vec<EvidenceRef>,
    pub(super) selected: usize,
    pub(super) limitation: Option<String>,
    pub(super) fit_pending: bool,
}

impl ShownEvidence {
    pub(super) fn identify_source(
        &self,
        document: &mut LoadedDocument,
        path: &std::path::Path,
        root: &std::path::Path,
    ) {
        if let Some(source) = self.source(path, root) {
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
    ) -> Vec<(SourceSide, review_source::SourceLineRange)> {
        let Some(comparison) = &self.comparison else {
            return Vec::new();
        };
        self.evidence
            .iter()
            .filter_map(|evidence| {
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

/// How the pane brought an evidence viewer to the front.
#[derive(Clone, Copy)]
pub(super) struct EvidenceShown {
    /// The viewer did not exist before.
    pub(super) opened: bool,
    /// Another viewer was in front before.
    pub(super) switched: bool,
}

impl SourceViewer {
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
            hunks: review_hunks::FileHunks::default(),
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
            if let Some(existing) = self.documents.iter_mut().find(|entry| entry.path == path) {
                *existing = document;
            } else {
                self.documents.push(document);
            }
        }
        if !self.documents.iter().any(|document| document.path == path) {
            self.documents
                .push(self.comparison_document(comparison, index));
        }
        self.comments.park_editor_outside(&path);
        self.selected_path = Some(path);
        self.preview = None;
        Ok(())
    }

    /// Show `comparison` from its first file, forgetting the previous one.
    pub(super) fn install_comparison(&mut self, comparison: Arc<Comparison>) {
        self.review_checkpoint = Some(comparison.checkpoint.clone());
        self.source_session = Some(format!("explore:{}", uuid::Uuid::new_v4()));
        self.documents = (0..comparison.files.len())
            .map(|index| self.comparison_document(&comparison, index))
            .collect();
        self.comments
            .use_comparison(ui_events::FileSummary::thread_paths(
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
            ));
        self.evidence.comparison = Some(comparison);
        self.evidence.evidence.clear();
        self.selected_path = None;
        self.preview = None;
        self.selection = None;
        self.search = diff_search::Search::default();
    }

    /// Show the evidence a question cites. A viewer that already showed it
    /// keeps its place unless `event` asks to reveal the evidence again.
    pub(super) fn show_question_evidence(
        &mut self,
        event: &ExploreEvidence,
        reference: usize,
        shown: EvidenceShown,
    ) -> Vec<Action> {
        self.evidence.comparison = Some(event.comparison.clone());
        self.evidence.evidence.clone_from(&event.evidence);
        if !shown.opened && !event.reveal {
            return self.retained_evidence_actions(shown.switched);
        }
        self.evidence.selected = reference;
        self.evidence.limitation = None;
        let Some(evidence) = event.evidence.get(reference) else {
            return Vec::new();
        };
        let cited = match event
            .comparison
            .cited_source(&evidence.location, &self.repository_root)
        {
            Ok(cited) => cited,
            Err(Uncitable::Path) => return Vec::new(),
            Err(limitation) => {
                self.evidence.limitation = Some(limitation.to_string());
                return Vec::new();
            }
        };
        let CitedSource {
            source,
            content,
            file,
        } = cited;
        if let Some(index) = file {
            return self.open_changed_evidence(
                &event.comparison,
                index,
                &source,
                &content,
                evidence,
            );
        }
        self.open_unchanged_evidence(&source, &content, evidence.location.lines.as_ref())
    }

    fn retained_evidence_actions(&mut self, switched: bool) -> Vec<Action> {
        let mut actions = self.request_visible_highlights();
        if switched {
            actions.extend(self.resume_evidence_search());
        }
        actions
    }

    /// Continue this viewer's search after another viewer was shown.
    pub(super) fn resume_evidence_search(&mut self) -> Vec<Action> {
        self.publish_search_status();
        let intents = self.search.resume(&self.documents);
        self.apply_search(intents)
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
            return self.open_unchanged_evidence(source, content, evidence.location.lines.as_ref());
        }
        if let Some(range) = &evidence.location.lines {
            let location = match source.side {
                SourceSide::Old => ui_events::PresentationLocation::OldLine(range.first_line - 1),
                SourceSide::New => ui_events::PresentationLocation::NewLine(range.first_line - 1),
            };
            self.reveal_evidence_range(source, content, range, location);
        }
        self.evidence.fit_pending = true;
        self.request_visible_highlights()
    }

    pub(super) fn explore_source(
        &mut self,
        location: review_lsp::SourceLocation,
        mode: ui_events::SourceLoadMode,
    ) -> Vec<Action> {
        let source = self.evidence.source(&location.path, &self.repository_root);
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
                self.comments.park_editor_outside(&document.path);
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
            .evidence
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
                hunks: review_hunks::FileHunks::default(),
            }));
            self.comments.park_editor_outside(&path);
        }
        actions
    }

    pub(super) fn explore_lsp_ready(&self) -> bool {
        if self.selected_document().is_some_and(|file| {
            file.document
                .diff
                .source_position(file.document.position().cursor())
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
