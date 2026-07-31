//! Filesystem-triggered repository refreshes.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(100);
const FALLBACK_POLL_INTERVAL: Duration = Duration::from_secs(30);
const FAILED_WATCHER_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub(super) struct RepositoryWatcher {
    watcher: Option<RecommendedWatcher>,
    events: Receiver<notify::Result<Event>>,
    next_poll: Instant,
    poll_interval: Duration,
}

impl RepositoryWatcher {
    pub(super) fn new(root: &Path) -> Self {
        let (sender, events) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })
        .and_then(|mut watcher| {
            watcher.watch(root, RecursiveMode::Recursive)?;
            Ok(watcher)
        })
        .ok();
        let poll_interval = if watcher.is_some() {
            FALLBACK_POLL_INTERVAL
        } else {
            FAILED_WATCHER_POLL_INTERVAL
        };

        Self {
            watcher,
            events,
            next_poll: Instant::now() + poll_interval,
            poll_interval,
        }
    }

    pub(super) fn poll_due(&mut self, now: Instant) -> bool {
        let mut notified = false;
        let mut failed = false;
        while let Ok(event) = self.events.try_recv() {
            notified = true;
            failed |= event.is_err();
        }

        if failed {
            // ponytail: Fall back to polling; restart only if transient failures matter.
            self.watcher = None;
            self.poll_interval = FAILED_WATCHER_POLL_INTERVAL;
            self.next_poll = now;
        } else if notified {
            self.next_poll = now + DEBOUNCE;
        }

        if now < self.next_poll {
            return false;
        }
        self.next_poll = now + self.poll_interval;
        true
    }
}

#[cfg(test)]
mod tests {
    use notify::EventKind;

    use super::*;

    #[test]
    fn notification_debounces_before_poll() {
        let (sender, events) = mpsc::channel();
        let now = Instant::now();
        let mut watcher = RepositoryWatcher {
            watcher: None,
            events,
            next_poll: now + FALLBACK_POLL_INTERVAL,
            poll_interval: FALLBACK_POLL_INTERVAL,
        };

        sender.send(Ok(Event::new(EventKind::Any))).unwrap();

        assert!(!watcher.poll_due(now));
        assert!(watcher.poll_due(now + DEBOUNCE));
        assert!(!watcher.poll_due(now + DEBOUNCE));
    }
}
