//! Past rounds: every saved round of the repository other than the one the page follows, which
//! the page lists in its ⋯ menu and opens read only. Only the page on this machine has them
//! (ADR 0003 and ADR 0004 open one round on the network and through the tunnel): its owner
//! attaches [`PastRounds`] to that page's round alone ([`crate::PageRound::with_past_rounds`]).
//!
//! The owner publishes the list ([`PastRoundsPublisher`]) whenever the saved rounds change, and
//! opens a round the reviewer asks for ([`PastRounds::new`]): it reads the round as it was saved,
//! and derives its stage as the page shows a round that offers no action ([`PastRound::new`]).
//! A page shows a past round on a socket of its own, which refuses every action.

use std::sync::Arc;

use review_repository::repository::ShortRevision;
use review_threads::ReviewThreads;
use serde::Serialize;
use tokio::sync::watch;
use ts_rs::TS;

use crate::conversation::ThreadsSnapshot;
use crate::round::{PublishedRound, ReviewName, RoundSnapshot, RoundStage};

/// One change of the repository with its saved rounds, as the ⋯ menu lists them.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
pub struct PastChange {
    /// The first line of the change's description as its newest round saved it; for a Git
    /// working tree, which has no description, "Git working tree".
    pub title: String,
    /// The change's abbreviated revision: a jj change, or the `HEAD` of a Git working tree.
    pub revision: ShortRevision,
    /// Its rounds, newest first.
    pub rounds: Vec<PastRoundEntry>,
}

/// A saved round of a change, as the ⋯ menu lists it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
pub struct PastRoundEntry {
    /// The round's identity, which the page's address names to open it.
    pub id: String,
    /// When the round was last saved, in milliseconds since the epoch.
    pub saved_at_ms: u64,
}

/// The saved rounds of the repository, as their owner last listed them.
#[derive(Clone, Debug, Default)]
struct PastList {
    /// Changes with every new list.
    revision: u64,
    /// The changes, the one with the newest round first.
    changes: Arc<[PastChange]>,
}

/// The owner's side of the list of saved rounds: publishes it to every page on this machine.
pub struct PastRoundsPublisher(watch::Sender<PastList>);

impl PastRoundsPublisher {
    /// Publishes `changes`, the changes with their rounds, the one with the newest round
    /// first. The same list again changes nothing.
    pub fn publish(&self, changes: Vec<PastChange>) {
        self.0.send_if_modified(|list| {
            if *list.changes == *changes {
                return false;
            }
            list.revision += 1;
            list.changes = changes.into();
            true
        });
    }
}

/// No round listed yet.
impl Default for PastRoundsPublisher {
    fn default() -> Self {
        Self(watch::Sender::new(PastList::default()))
    }
}

/// Opens the saved round of an identity, or `None` when no saved round has it. It reads the
/// round from disk, so it blocks.
type Open = Arc<dyn Fn(&str) -> Option<PastRound> + Send + Sync>;

/// The saved rounds of the repository, as a page on this machine reaches them: their list, and
/// a way to open one.
#[derive(Clone)]
pub struct PastRounds {
    list: watch::Receiver<PastList>,
    open: Open,
}

impl PastRounds {
    /// The rounds `publisher` lists, each of which `open` reads and derives, blocking.
    pub fn new(
        publisher: &PastRoundsPublisher,
        open: impl Fn(&str) -> Option<PastRound> + Send + Sync + 'static,
    ) -> Self {
        Self {
            list: publisher.0.subscribe(),
            open: Arc::new(open),
        }
    }

    /// The revision of the latest list.
    pub(crate) fn revision(&self) -> u64 {
        self.list.borrow().revision
    }

    /// The changes of the latest list, without the round `followed`, which the page follows:
    /// a change left with no round is left out.
    pub(crate) fn listed(&self, followed: Option<&str>) -> Vec<PastChange> {
        let changes = self.list.borrow().changes.clone();
        changes
            .iter()
            .filter_map(|change| {
                let rounds: Vec<_> = change
                    .rounds
                    .iter()
                    .filter(|round| Some(round.id.as_str()) != followed)
                    .cloned()
                    .collect();
                (!rounds.is_empty()).then(|| PastChange {
                    rounds,
                    ..change.clone()
                })
            })
            .collect()
    }

    /// When the latest list says the round `id` was last saved; `None` when it lists no such
    /// round.
    pub(crate) fn saved_at(&self, id: &str) -> Option<u64> {
        self.list
            .borrow()
            .changes
            .iter()
            .flat_map(|change| &change.rounds)
            .find(|round| round.id == id)
            .map(|round| round.saved_at_ms)
    }

    /// Opens the round `id` on a thread that may block, or `None` when no saved round has it.
    pub(crate) async fn open(&self, id: String) -> Option<PastRound> {
        let open = self.open.clone();
        tokio::task::spawn_blocking(move || open(&id))
            .await
            .ok()
            .flatten()
    }

    /// Waits for the next list. Returns false once the publisher is gone.
    pub(crate) async fn changed(&mut self) -> bool {
        self.list.changed().await.is_ok()
    }
}

/// A saved round, as the page shows it: read only, as it was last saved.
#[derive(Clone, Debug)]
pub struct PastRound {
    pub(crate) snapshot: RoundSnapshot,
    /// The change the round reviewed.
    review: ReviewName,
    /// The threads of the round's review, which hold its conversation.
    pub(crate) threads: ThreadsSnapshot,
    /// When the round was last saved, in milliseconds since the epoch.
    pub(crate) saved_at_ms: u64,
}

impl PastRound {
    /// The round `round` at `stage`, of the change `review`, whose conversation the threads
    /// `threads` of its review hold, last saved at `saved_at_ms`. The page offers no action on
    /// it, whatever `round` says.
    pub fn new(
        round: PublishedRound<'_>,
        stage: RoundStage,
        review: ReviewName,
        threads: Option<ReviewThreads>,
        saved_at_ms: u64,
    ) -> Self {
        let round = PublishedRound {
            cancellable: None,
            earlier: true,
            ..round
        };
        Self {
            snapshot: RoundSnapshot::past(round, stage, review.clone()),
            review,
            threads: ThreadsSnapshot::of(threads),
            saved_at_ms,
        }
    }

    /// The round's identity.
    pub fn id(&self) -> &str {
        self.snapshot.round.as_deref().unwrap_or_default()
    }

    /// Where the round stopped, as the page shows it.
    pub fn stage(&self) -> &RoundStage {
        &self.snapshot.stage
    }

    /// The change the round reviewed.
    pub fn review(&self) -> &ReviewName {
        &self.review
    }

    /// When the round was last saved, in milliseconds since the epoch.
    pub fn saved_at_ms(&self) -> u64 {
        self.saved_at_ms
    }
}
