//! The brief progress-bar tail is derived from the saved filtering stop time.
use review_explore::CoverageLedger;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROGRESS_TAIL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct JevProgressExpiry {
    stopped_at_ms: Option<u64>,
    deadline: Option<Instant>,
}

impl JevProgressExpiry {
    pub(super) fn observe(&mut self, coverage: &CoverageLedger) {
        if self.stopped_at_ms == coverage.classification_stopped_at_ms {
            return;
        }
        self.stopped_at_ms = coverage.classification_stopped_at_ms;
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

    pub(super) fn visible(&self, coverage: &CoverageLedger) -> bool {
        if coverage.classification_stopped_at_ms.is_none() {
            // Older saved passes have no stop time; a completed bar is already stale.
            return !coverage.classification_finished;
        }
        self.deadline
            .is_some_and(|deadline| Instant::now() < deadline)
    }

    pub(super) fn changes_between(&self, previous: Instant, now: Instant) -> bool {
        self.deadline
            .is_some_and(|deadline| previous < deadline && deadline <= now)
    }
}
