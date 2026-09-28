//! Explicit Jev classification and review marks for one immutable comparison.

use super::{ApplicationMessageSender, Worker, WorkerCommand, jev};
use review_explore::{Comparison, CoverageLedger, Significance};
use review_repository::repository::{PollResult, Repository};
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
    comparison: Comparison,
    coverage: CoverageLedger,
    prior_marks: Vec<LoadResult>,
    active: Arc<AtomicBool>,
    finished: bool,
}

struct MarkSummary {
    marked: usize,
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
            coverage: CoverageLedger::new(&comparison),
            comparison,
            prior_marks,
            active: Arc::new(AtomicBool::new(true)),
            finished: false,
        })
    }

    fn classify(&mut self, key: &str) {
        let candidates = jev::Candidate::prepare(&self.comparison);
        self.finished = jev::classify(key, candidates, |result| {
            if !self.active.load(Ordering::Relaxed) {
                return false;
            }
            self.coverage.record_significance(result);
            true
        });
    }

    fn apply(&self, repository: &Repository, store: &ReviewStore) -> eyre::Result<MarkSummary> {
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
            remaining: self.comparison.files.len(),
            failed_classifications: self
                .coverage
                .classifications
                .values()
                .filter(|result| {
                    matches!(
                        result.outcome,
                        Significance::Failed | Significance::Oversized
                    )
                })
                .count(),
            failed_marks: 0,
        };
        for index in self.coverage.fully_excluded_files(&self.comparison) {
            // Preserve manual marks and unknown schemas, including writes by another reviewer.
            let path = self.comparison.files[index].review_path().as_bytes();
            let prior = &self.prior_marks[index];
            match store.load(&checkpoint.review_unit, path) {
                Ok(current) if current == *prior && current != LoadResult::UnknownSchema => {
                    if store
                        .mark(&checkpoint.review_unit, path, &checkpoint.checkpoint)
                        .is_ok()
                    {
                        summary.marked += 1;
                        summary.remaining -= 1;
                    } else {
                        summary.failed_marks += 1;
                    }
                }
                Err(_) => summary.failed_marks += 1,
                Ok(_) => {}
            }
        }
        Ok(summary)
    }
}

impl MarkSummary {
    fn toast(&self) -> ui_events::ToastRequested {
        let mut text = format!(
            "Jev: marked {} files reviewed; {} still need review.",
            self.marked, self.remaining
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
        messages: &ApplicationMessageSender,
    ) {
        let result = self.prepare_auto_review(checkpoint);
        let toast = match result {
            Ok((mut review, key)) => {
                let total = review.comparison.files.len();
                self.auto_review = Some(review.active.clone());
                let commands = self.commands.clone();
                std::thread::spawn(move || {
                    review.classify(&key);
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
    ) -> eyre::Result<(AutoReview, String)> {
        eyre::ensure!(
            self.auto_review.is_none(),
            "Jev automatic review is already running"
        );
        let key = jev::key().ok_or_else(|| {
            eyre::eyre!("Set TYPESAFE_API_KEY to automatically review files with Jev")
        })?;
        let review = AutoReview::prepare(&self.repository, &self.tracker, &self.store, checkpoint)?;
        eyre::ensure!(
            !review.comparison.files.is_empty(),
            "All files are already reviewed"
        );
        Ok((review, key))
    }

    pub(super) fn finish_auto_review(
        &mut self,
        review: &AutoReview,
        messages: &ApplicationMessageSender,
    ) {
        self.auto_review = None;
        let toast = match review.apply(&self.repository, &self.store) {
            Ok(summary) => {
                self.poll(messages);
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
