//! Discarding forks: each is stopped, its transcript deleted, and its record says why. Forks go
//! when their question no longer waits, when the agent's session moved, when run-ahead is
//! turned off and when the reviewer closes; a reopened reviewer discards those a stopped
//! reviewer left.

use std::collections::HashSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use review_run_ahead::{DiscardReason, ForkEnd, ForkPoint, ForkTrace};

use super::{Event, RoundKey, RunAheadInput, TakenFork};
use crate::{ExploreSession, Input};

/// How long a closing reviewer waits for its forks to stop: SIGTERM, three seconds, SIGKILL.
const CLOSE_WAIT: Duration = Duration::from_secs(6);

impl ExploreSession {
    /// Stops watching the question that waits, and discards its forks for `reason`.
    pub(crate) fn run_ahead_discard(&mut self, reason: DiscardReason) {
        if self.run_ahead.armed.is_none() {
            return;
        }
        self.discard_taken(reason);
        self.run_ahead.armed = None;
    }

    /// Discards the forks taken for the question that waits, for `reason`, and keeps watching.
    pub(super) fn discard_taken(&mut self, reason: DiscardReason) {
        let Some(armed) = self.run_ahead.armed.as_mut() else {
            return;
        };
        let Some(taken) = armed.taken.take() else {
            return;
        };
        let round = armed.asked.round.clone();
        self.discard_forks(&round, &taken.point, &taken.forks, reason);
    }

    /// Discards `forks` of `round`, taken from `point`, for `reason`: each is stopped and its
    /// transcript deleted.
    pub(super) fn discard_forks(
        &mut self,
        round: &RoundKey,
        point: &ForkPoint,
        forks: &[TakenFork],
        reason: DiscardReason,
    ) {
        self.mark_discarded(round, forks, reason);
        for fork in forks {
            self.discard_in_background(round, fork.trace(point));
        }
        self.run_ahead
            .log(&format!("{} forks discarded: {reason:?}", forks.len()));
    }

    /// Stops the fork `fork` of `round` and deletes its transcript; its record says so once
    /// both are done.
    fn discard_in_background(&self, round: &RoundKey, fork: ForkTrace<'_>) {
        let inbox = self.inbox.clone();
        let cleaned = Event::Cleaned {
            round: round.clone(),
            session: fork.session.to_owned(),
        };
        self.run_ahead.host.discard(
            fork,
            Box::new(move || inbox.deliver(Input::RunAhead(RunAheadInput(cleaned)))),
        );
    }

    /// Records `forks` as discarded for `reason`, and refuses their calls from now on.
    fn mark_discarded(&mut self, round: &RoundKey, forks: &[TakenFork], reason: DiscardReason) {
        let sessions: HashSet<_> = forks.iter().map(|fork| &fork.session).collect();
        self.run_ahead
            .discarded
            .extend(forks.iter().map(|fork| fork.access.clone()));
        let now = review_explore::now_ms();
        let saved = self.update_forks(round, |forks| {
            for record in &mut forks.forks {
                if sessions.contains(&record.session) {
                    record.discard(reason, now);
                }
            }
        });
        if let Err(error) = saved {
            self.run_ahead
                .log(&format!("the discarded forks were not recorded: {error}"));
        }
    }

    /// The reviewer closes: discards the forks of the question that waits and waits for them
    /// to stop, at most six seconds. The host lets the discards already under way finish.
    pub(crate) fn run_ahead_close(&mut self) {
        let Some(mut armed) = self.run_ahead.armed.take() else {
            return;
        };
        let Some(taken) = armed.taken.take() else {
            return;
        };
        self.mark_discarded(
            &armed.asked.round,
            &taken.forks,
            DiscardReason::ReviewerClosed,
        );
        let (done, stopped) = mpsc::channel();
        for fork in &taken.forks {
            let done = done.clone();
            let session = fork.session.clone();
            self.run_ahead.host.discard(
                fork.trace(&taken.point),
                Box::new(move || drop(done.send(session))),
            );
        }
        drop(done);
        let deadline = Instant::now() + CLOSE_WAIT;
        let mut cleaned = HashSet::new();
        while cleaned.len() < taken.forks.len() {
            let left = deadline.saturating_duration_since(Instant::now());
            match stopped.recv_timeout(left) {
                Ok(session) => {
                    cleaned.insert(session);
                }
                Err(_) => break,
            }
        }
        let _ = self.update_forks(&armed.asked.round, |forks| {
            for record in &mut forks.forks {
                record.cleaned |= cleaned.contains(&record.session);
            }
        });
        self.run_ahead.host.log(&format!(
            "the reviewer closed: {} forks stopped",
            cleaned.len()
        ));
    }

    /// The reviewer opened the review: discards the forks of its rounds that a stopped reviewer
    /// left running or on disk. Forks of a reviewer that still runs stay its own.
    pub(crate) fn run_ahead_restore(&mut self) {
        let Some(unit) = self.state.loaded_unit.clone() else {
            return;
        };
        let Ok(history) = self.rounds.history(&unit) else {
            return;
        };
        for instance in history.rounds {
            let round = RoundKey {
                unit: unit.clone(),
                instance,
            };
            self.discard_left(&round);
        }
    }

    /// Discards the forks of `round` that a stopped reviewer left.
    fn discard_left(&mut self, round: &RoundKey) {
        let reviewer = self.run_ahead.reviewer;
        let now = review_explore::now_ms();
        let left = self.update_forks(round, |forks| {
            forks
                .forks
                .iter_mut()
                .filter(|record| {
                    !record.cleaned
                        && !record.is_agent_session()
                        && record.reviewer != reviewer
                        && !record.reviewer.is_running()
                })
                .map(|record| {
                    record.discard(DiscardReason::ReviewerStopped, now);
                    record.clone()
                })
                .collect::<Vec<_>>()
        });
        let Ok(left) = left else {
            return;
        };
        for record in &left {
            self.discard_in_background(round, record.trace());
        }
        if !left.is_empty() {
            self.run_ahead.host.log(&format!(
                "cleaning up {} forks a stopped reviewer left",
                left.len()
            ));
        }
    }

    /// The fork `session` of `round` ended: its record keeps how, and its tokens.
    pub(super) fn run_ahead_ended(&mut self, round: &RoundKey, session: &str, end: &ForkEnd) {
        if let Some(fork) = self.run_ahead.fork_mut(session) {
            fork.ended = true;
        }
        let saved = self.update_forks(round, |forks| {
            let record = forks.fork_mut(session)?;
            record.exit = Some(end.exit.clone());
            record.usage = Some(end.usage);
            Some(record.choice.clone())
        });
        if let Ok(Some(choice)) = saved {
            let usage = end.usage;
            self.run_ahead.host.log(&format!(
                "the fork for {choice} ({session}) ended ({}{}): {} input, {} cache write, {} cache read, {} output tokens",
                end.exit,
                if end.finished { ", its turn done" } else { "" },
                usage.input,
                usage.cache_creation,
                usage.cache_read,
                usage.output,
            ));
        }
    }

    /// The fork `session` of `round` is stopped and its transcript deleted.
    pub(super) fn run_ahead_cleaned(&mut self, round: &RoundKey, session: &str) {
        let _ = self.update_forks(round, |forks| {
            if let Some(record) = forks.fork_mut(session) {
                record.cleaned = true;
            }
        });
    }
}
