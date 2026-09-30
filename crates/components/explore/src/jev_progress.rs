//! The brief progress-bar tail is derived from the saved filtering stop time.
use review_explore::{ClassificationProgress, ClassificationState, CoverageLedger};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROGRESS_TAIL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct JevProgressExpiry {
    stopped_at_ms: Option<u64>,
    deadline: Option<Instant>,
}

impl JevProgressExpiry {
    pub(super) fn observe(&mut self, coverage: &CoverageLedger) {
        let stopped_at_ms = coverage
            .classification_progress()
            .and_then(|progress| progress.state.ended_at_ms());
        if self.stopped_at_ms == stopped_at_ms {
            return;
        }
        self.stopped_at_ms = stopped_at_ms;
        self.deadline = self.stopped_at_ms.and_then(|stopped_at_ms| {
            let now_ms = u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            )
            .unwrap_or(u64::MAX);
            let elapsed = Duration::from_millis(now_ms.saturating_sub(stopped_at_ms));
            PROGRESS_TAIL
                .checked_sub(elapsed)
                .map(|remaining| Instant::now() + remaining)
        });
    }

    pub(super) fn visible(&self, progress: ClassificationProgress) -> bool {
        match progress.state {
            ClassificationState::Running => true,
            // Older saved passes have no stop time; a completed bar is already stale.
            ClassificationState::Finished { at_ms: None } => false,
            ClassificationState::Finished { at_ms: Some(_) }
            | ClassificationState::Stopped { .. } => self
                .deadline
                .is_some_and(|deadline| Instant::now() < deadline),
        }
    }

    pub(super) fn changes_between(&self, previous: Instant, now: Instant) -> bool {
        self.deadline
            .is_some_and(|deadline| previous < deadline && deadline <= now)
    }
}
