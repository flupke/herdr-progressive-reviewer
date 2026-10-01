//! Explicit Jev classification and review marks for one immutable comparison.

use super::worker::{Worker, WorkerCommand};
use component_core::ApplicationEventSender;
use review_explore::{
    Comparison, CoverageLedger, ExcludedLines, ExclusionPolicy, Significance,
    SignificanceClassifier, SourceSide,
};
use review_hunks::ChangedLines;
use review_repository::repository::{ChangedFile, PollResult, Repository, Snapshot};
use review_source::ReviewCheckpoint;
use review_state::ReviewTracker;
use review_store::{LoadResult, ReviewStore};
use std::fmt::Write;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug)]
pub(super) struct AutoReview {
    /// The classified files, at the classified commit.
    snapshot: Snapshot,
    comparison: Comparison,
    coverage: CoverageLedger,
    prior_marks: Vec<LoadResult>,
    active: Arc<AtomicBool>,
    finished: bool,
}

struct MarkSummary {
    marked: usize,
    hunks: usize,
    /// Paths whose review mark changed.
    changed: Vec<String>,
    remaining: usize,
    failed_classifications: usize,
    failed_marks: usize,
}

impl AutoReview {
    fn prepare(
        repository: &Repository,
        tracker: &ReviewTracker,
        store: &ReviewStore,
        checkpoint: &ReviewCheckpoint,
    ) -> eyre::Result<Self> {
        let PollResult::Complete(mut snapshot) = repository.poll()? else {
            eyre::bail!("Comparison changed; retry rf after refresh");
        };
        eyre::ensure!(
            snapshot.identity.review_unit() == &checkpoint.review_unit
                && snapshot.identity.snapshot_id() == checkpoint.checkpoint,
            "Comparison changed; retry rf after refresh"
        );
        let states = tracker.statuses(&snapshot)?;
        snapshot.files = snapshot
            .files
            .into_iter()
            .zip(states)
            .filter_map(|(file, state)| state.status.needs_review().then_some(file))
            .collect();
        let comparison = Comparison::prepare(repository, &snapshot)?;
        let prior_marks = comparison
            .files
            .iter()
            .map(|file| store.load(&checkpoint.review_unit, file.review_path().as_bytes()))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            snapshot,
            coverage: CoverageLedger::new(&comparison),
            comparison,
            prior_marks,
            active: Arc::new(AtomicBool::new(true)),
            finished: false,
        })
    }

    fn classify(&mut self, classifier: &dyn SignificanceClassifier) {
        let plan = classifier.plan(&self.comparison, &|_| false);
        self.finished = plan.run(|result| {
            if !self.active.load(Ordering::Relaxed) {
                return false;
            }
            self.coverage.record_significance(result);
            true
        });
    }

    fn apply(
        &self,
        repository: &Repository,
        tracker: &ReviewTracker,
        store: &ReviewStore,
    ) -> eyre::Result<MarkSummary> {
        eyre::ensure!(
            self.active.load(Ordering::Relaxed),
            "Automatic review cancelled because the comparison or review marks changed"
        );
        eyre::ensure!(self.finished, "Jev classification stopped; retry rf");
        let checkpoint = &self.comparison.checkpoint;
        let current = repository.current_identity()?;
        eyre::ensure!(
            current.review_unit() == &checkpoint.review_unit
                && current.snapshot_id() == checkpoint.checkpoint,
            "Comparison changed; no files marked. Retry rf after refresh"
        );
        let mut summary = MarkSummary {
            marked: 0,
            hunks: 0,
            changed: Vec::new(),
            remaining: self.comparison.files.len(),
            failed_classifications: self
                .coverage
                .classifications()
                .filter(|result| {
                    matches!(
                        result.outcome,
                        Significance::Failed | Significance::Oversized
                    )
                })
                .count(),
            failed_marks: 0,
        };
        let whole_files = self.coverage.fully_excluded_files(&self.comparison);
        for (index, file) in self.snapshot.files.iter().enumerate() {
            // Preserve manual marks and unknown schemas, including writes by another reviewer.
            match self.unchanged_mark(store, index) {
                Ok(false) => {}
                Ok(true) if whole_files.contains(&index) => {
                    let marked = store.mark(
                        &checkpoint.review_unit,
                        file.review_path().as_bytes(),
                        &checkpoint.checkpoint,
                    );
                    summary.record_file(file, marked.is_ok());
                }
                Ok(true) => summary.record_hunks(file, self.review_hunks(tracker, index, file)),
                Err(_) => summary.failed_marks += 1,
            }
        }
        Ok(summary)
    }

    /// Whether the file's review mark is still the one classification started from.
    fn unchanged_mark(&self, store: &ReviewStore, index: usize) -> eyre::Result<bool> {
        let path = self.comparison.files[index].review_path().as_bytes();
        let current = store.load(&self.comparison.checkpoint.review_unit, path)?;
        Ok(current == self.prior_marks[index] && current != LoadResult::UnknownSchema)
    }

    /// Accept the open hunks of a file whose every changed line Jev excluded,
    /// or `None` when they could not be saved. A file that changes more than
    /// its lines keeps at least one hunk open, since only a whole-file review
    /// covers the rest.
    fn review_hunks(
        &self,
        tracker: &ReviewTracker,
        index: usize,
        file: &ChangedFile,
    ) -> Option<usize> {
        let excluded = self.coverage.excluded_lines(index);
        if excluded.is_empty() {
            return Some(0);
        }
        let may_review_file = !self
            .coverage
            .changes_more_than_lines(&self.comparison, index);
        tracker
            .review_hunks_where(&self.snapshot, file, may_review_file, |lines| {
                insignificant(&excluded, lines)
            })
            .ok()
    }
}

/// Whether Jev excluded every line an open hunk changes. A hunk that rewrites
/// approved lines stays open: Jev judged its lines against the base, not
/// against what the reviewer approved.
fn insignificant(excluded: &ExcludedLines, lines: &ChangedLines) -> bool {
    // Diff rows number lines from one.
    let contains = |side, line: &u32| excluded.contains(side, line + 1);
    if lines.rewrites_reviewed || (lines.removed.is_empty() && lines.added.is_empty()) {
        return false;
    }
    lines
        .removed
        .iter()
        .all(|line| contains(SourceSide::Old, line))
        && lines
            .added
            .iter()
            .all(|line| contains(SourceSide::New, line))
}

impl MarkSummary {
    fn record_file(&mut self, file: &ChangedFile, marked: bool) {
        if marked {
            self.marked += 1;
            self.remaining -= 1;
            self.changed.push(file.review_path().display());
        } else {
            self.failed_marks += 1;
        }
    }

    fn record_hunks(&mut self, file: &ChangedFile, accepted: Option<usize>) {
        match accepted {
            Some(0) => {}
            Some(hunks) => {
                self.hunks += hunks;
                self.changed.push(file.review_path().display());
            }
            None => self.failed_marks += 1,
        }
    }

    fn toast(&self) -> ui_events::ToastRequested {
        let mut text = format!(
            "Jev: marked {} files and {} hunks reviewed; {} files still need review.",
            self.marked, self.hunks, self.remaining
        );
        if self.failed_classifications > 0 {
            let _ = write!(
                text,
                " {} classifications failed or exceeded limits.",
                self.failed_classifications
            );
        }
        if self.failed_marks > 0 {
            let _ = write!(
                text,
                " {} review marks could not be saved.",
                self.failed_marks
            );
        }
        ui_events::ToastRequested {
            text,
            kind: if self.failed_marks > 0 || self.failed_classifications > 0 {
                toasts::ToastKind::Error
            } else {
                toasts::ToastKind::Info
            },
        }
    }
}

impl Worker {
    pub(super) fn start_auto_review(
        &mut self,
        checkpoint: &ReviewCheckpoint,
        messages: &ApplicationEventSender,
    ) {
        let result = self.prepare_auto_review(checkpoint);
        let toast = match result {
            Ok((mut review, exclusion)) => {
                let total = review.comparison.files.len();
                self.auto_review = Some(review.active.clone());
                let commands = self.commands.clone();
                std::thread::spawn(move || {
                    if let Some(classifier) = exclusion.classifier() {
                        review.classify(classifier);
                    }
                    let _ = commands.send(WorkerCommand::AutoReviewFinished(Box::new(review)));
                });
                ui_events::ToastRequested {
                    text: format!("Jev: classifying changes in {total} files…"),
                    kind: toasts::ToastKind::Info,
                }
            }
            Err(error) => ui_events::ToastRequested {
                text: error.to_string(),
                kind: toasts::ToastKind::Error,
            },
        };
        let _ = messages.send(toast);
    }

    fn prepare_auto_review(
        &self,
        checkpoint: &ReviewCheckpoint,
    ) -> eyre::Result<(AutoReview, ExclusionPolicy)> {
        eyre::ensure!(
            self.auto_review.is_none(),
            "Jev automatic review is already running"
        );
        eyre::ensure!(
            self.exclusion.is_enabled(),
            "Set TYPESAFE_API_KEY to automatically review files with Jev"
        );
        let review = AutoReview::prepare(&self.repository, &self.tracker, &self.store, checkpoint)?;
        eyre::ensure!(
            !review.comparison.files.is_empty(),
            "All files are already reviewed"
        );
        Ok((review, self.exclusion.clone()))
    }

    pub(super) fn finish_auto_review(
        &mut self,
        review: &AutoReview,
        messages: &ApplicationEventSender,
    ) {
        self.auto_review = None;
        let toast = match review.apply(&self.repository, &self.tracker, &self.store) {
            Ok(summary) => {
                self.poll(messages);
                // The comparison is unchanged, so loaded diffs learn of the
                // new marks only from these states.
                self.announce_review_states(messages, |file| {
                    summary.changed.contains(&file.review_path().display())
                });
                summary.toast()
            }
            Err(error) => ui_events::ToastRequested {
                text: error.to_string(),
                kind: toasts::ToastKind::Error,
            },
        };
        let _ = messages.send(toast);
    }

    pub(super) fn cancel_auto_review(&self) {
        if let Some(active) = &self.auto_review {
            active.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests;
