//! Reveal complete evidence ranges, including historical lines outside diff hunks.
use super::{DiffComponent, LoadedDocument, presentation::DiffPresentation};
use review_explore::{Source, SourceSide};
use review_guide::GuideLineRange;
use std::{os::unix::ffi::OsStrExt, sync::Arc};
use ui_actions::Action;
use ui_events::{DiffContentLoaded, HighlightRequest, PresentationLocation};

impl DiffComponent {
    pub(super) fn open_supporting_evidence(
        &mut self,
        source: &Source,
        content: &str,
        lines: Option<&GuideLineRange>,
    ) -> Vec<Action> {
        let line = lines.map_or(0, |range| range.first_line - 1);
        if source.side == SourceSide::Old {
            let path = source.display_path.clone();
            if !self.documents.iter().any(|document| document.path == path) {
                self.documents.push(LoadedDocument::new(path.clone()));
            }
            self.comments.restore_file_editor(&path);
            self.selected_path = Some(path);
            self.preview = None;
            self.show_base_evidence(source, content, line);
            self.explore.fit_pending = true;
            return self.request_visible_highlights();
        }
        let actions = self.explore_source(
            review_lsp::SourceLocation {
                path: self
                    .repository_root
                    .join(std::ffi::OsStr::from_bytes(source.path.as_bytes())),
                line,
                byte_column: 0,
                end_line: line,
                end_byte_column: 0,
            },
            ui_events::SourceLoadMode::External,
        );
        self.explore.fit_pending = true;
        actions
    }

    pub(super) fn reveal_evidence_range(
        &mut self,
        source: &Source,
        content: &str,
        range: &GuideLineRange,
        first: PresentationLocation,
    ) {
        let missing = !self.contains_evidence_range(source.side, range);
        if source.side == SourceSide::Old && missing {
            self.show_base_evidence(source, content, range.first_line - 1);
            return;
        }
        let document = self.selected_document_mut().expect("evidence document");
        if missing {
            document.document.diff.show_file();
        }
        if let Some(row) = document.document.diff.reveal_presentation_location(first) {
            document.document.cursor = row;
            document.document.column = 0;
        }
    }

    fn contains_evidence_range(&self, side: SourceSide, range: &GuideLineRange) -> bool {
        let document = &self
            .selected_document()
            .expect("evidence document")
            .document;
        let lines = (0..document.diff.len())
            .filter_map(|row| document.diff.evidence_line(row, side))
            .filter(|line| range.first_line <= *line && *line <= range.last_line)
            .count();
        lines as u64 == u64::from(range.last_line - range.first_line) + 1
    }

    fn show_base_evidence(&mut self, source: &Source, content: &str, line: u32) {
        let content = Arc::new(DiffContentLoaded {
            review_checkpoint: self
                .explore
                .comparison
                .as_ref()
                .expect("comparison")
                .checkpoint
                .clone(),
            path: self.selected_path.clone().expect("evidence path"),
            rows: Vec::new(),
            old_content: Some(content.as_bytes().to_vec()),
            new_content: None,
        });
        let mut document = LoadedDocument::new(self.selected_path.clone().expect("evidence path"));
        document.display_path.clone_from(&source.display_path);
        document.old_path = Some(source.display_path.clone());
        document.temporary = true;
        document.replace_diff(DiffPresentation::base_file(self.highlighter.plain(
            Vec::new(),
            content.old_content.as_deref(),
            None,
        )));
        document.document.cursor = line as usize;
        document
            .document
            .prepare_highlighting(HighlightRequest::Diff(content.clone()));
        document.content = Some(content);
        let selected = self.selected_document_mut().expect("evidence document");
        *selected = document;
        self.comments.refresh_anchors(&self.documents);
    }
}
