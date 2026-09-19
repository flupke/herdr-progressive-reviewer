//! Reopen only the selected evidence file, using the existing working-copy/history readers.
use crate::{DiffComponent, LoadedDocument};
use review_explore::{CodeLocation, Comparison, SourceSide};
use review_repository::{
    diff::parse_file_diff,
    repository::{Repository, Snapshot},
};
use std::sync::Arc;

impl DiffComponent {
    pub(super) fn restored_comparison_document(
        &self,
        comparison: &Comparison,
        index: usize,
    ) -> eyre::Result<LoadedDocument> {
        let file = &comparison.files[index];
        let snapshot = Snapshot {
            identity: comparison
                .base
                .clone()
                .ok_or_else(|| eyre::eyre!("Comparison base is unavailable"))?,
            files: vec![file.clone()],
        };
        let repository = Repository::discover(&self.repository_root)?;
        let diff = repository.diff(&snapshot, file)?;
        let read =
            |path: &Option<review_repository::repository::RepoPath>, side| -> eyre::Result<_> {
                path.as_ref()
                    .map(|path| {
                        comparison
                            .source(&CodeLocation {
                                path: path.clone(),
                                side,
                                lines: None,
                            })
                            .ok_or_else(|| eyre::eyre!("Invalid evidence source"))?
                            .read(&self.repository_root)
                    })
                    .transpose()
            };
        let loaded = Arc::new(ui_events::DiffContentLoaded {
            review_checkpoint: comparison.checkpoint.clone(),
            path: file.review_path().display(),
            rows: parse_file_diff(&diff, file),
            old_content: read(&file.old_path, SourceSide::Old)?,
            new_content: read(&file.new_path, SourceSide::New)?,
        });
        Ok(self.comparison_content_document(file, loaded))
    }
}
