//! The round's conversation on the page: the review thread attached to the round
//! (`review_round_conversation`), as the owner of the review threads publishes the threads, and
//! the reviewer's messages, which the page sends that owner as thread commands. The conversation
//! is a review thread like any other: the page stores nothing of it, and a message is never an
//! Explore turn.

use std::collections::HashMap;
use std::sync::Arc;

use review_round_conversation::RoundConversation;
use review_threads::{MessageId, ReviewThreads, ThreadCommand, WakeupFailure};
use review_types::ReviewUnit;
use tokio::sync::watch;

use crate::command::{CommandReply, await_reply};
use crate::notice::Problem;

/// The review threads as their owner holds them, for the pages: the latest threads of each
/// review, and what became of the latest wakeup for each review's pending comments.
#[derive(Clone, Debug, Default)]
pub(crate) struct ThreadsSnapshot {
    /// Changes with every change the pages show.
    pub(crate) revision: u64,
    by_review: HashMap<ReviewUnit, Arc<ReviewThreads>>,
    failures: HashMap<ReviewUnit, WakeupFailure>,
}

impl ThreadsSnapshot {
    /// The threads of the review `unit`, once their owner loaded them.
    pub(crate) fn threads(&self, unit: &ReviewUnit) -> Option<&ReviewThreads> {
        self.by_review.get(unit).map(Arc::as_ref)
    }

    /// Whether the threads of the review `unit` hold the message `id`.
    pub(crate) fn holds(&self, unit: &ReviewUnit, id: &MessageId) -> bool {
        self.threads(unit)
            .is_some_and(|threads| threads.message(id).is_some())
    }

    /// The conversation of the round `round` of the review `unit`: empty until the owner loaded
    /// the review's threads, or until the round's first message.
    pub(crate) fn conversation(&self, unit: &ReviewUnit, round: &str) -> RoundConversation {
        let failure = self.failures.get(unit);
        match self.threads(unit) {
            Some(threads) => RoundConversation::new(threads, round, failure),
            None => RoundConversation::new(&ReviewThreads::new(unit.clone()), round, failure),
        }
    }
}

/// The owner's side of the review threads: publishes them to every page.
pub struct ThreadsPublisher(watch::Sender<ThreadsSnapshot>);

impl ThreadsPublisher {
    /// The owner loaded or changed `threads`, the whole threads of their review. The same
    /// threads again change nothing.
    pub fn loaded(&self, threads: ReviewThreads) {
        self.0.send_if_modified(|snapshot| {
            if snapshot.threads(&threads.review_unit) == Some(&threads) {
                return false;
            }
            snapshot.revision += 1;
            snapshot
                .by_review
                .insert(threads.review_unit.clone(), Arc::new(threads));
            true
        });
    }

    /// The latest wakeup for the pending comments of the review `unit` is on its way to the
    /// agent (`None`), or did not reach it, for `failure`. The same again changes nothing.
    pub fn wakeup(&self, unit: ReviewUnit, failure: Option<WakeupFailure>) {
        self.0.send_if_modified(|snapshot| {
            if snapshot.failures.get(&unit) == failure.as_ref() {
                return false;
            }
            snapshot.revision += 1;
            match failure {
                Some(failure) => snapshot.failures.insert(unit, failure),
                None => snapshot.failures.remove(&unit),
            };
            true
        });
    }

    pub fn subscribe(&self) -> ThreadsFeed {
        ThreadsFeed(self.0.subscribe())
    }
}

/// No threads loaded yet.
impl Default for ThreadsPublisher {
    fn default() -> Self {
        Self(watch::Sender::new(ThreadsSnapshot::default()))
    }
}

/// The page's side of the review threads: the latest the owner published.
#[derive(Clone, Debug)]
pub struct ThreadsFeed(watch::Receiver<ThreadsSnapshot>);

impl ThreadsFeed {
    pub(crate) fn latest(&self) -> ThreadsSnapshot {
        self.0.borrow().clone()
    }

    /// The conversation of the round `round` of the review `unit`, as the latest threads hold
    /// it.
    pub fn conversation(&self, unit: &ReviewUnit, round: &str) -> RoundConversation {
        self.0.borrow().conversation(unit, round)
    }

    /// Waits for the next change. Returns false once the publisher is gone.
    pub(crate) async fn changed(&mut self) -> bool {
        self.0.changed().await.is_ok()
    }
}

/// Where a page sends the reviewer's commands on the round's conversation: to the owner of the
/// review threads, the one that carries out the pane's, which replies to each once it carried it
/// out (a post, once the threads hold it).
#[derive(Clone)]
pub struct ThreadSender(Arc<dyn Fn(ThreadCommand, CommandReply) + Send + Sync>);

impl ThreadSender {
    pub fn new(deliver: impl Fn(ThreadCommand, CommandReply) + Send + Sync + 'static) -> Self {
        Self(Arc::new(deliver))
    }

    /// Sends `command` to the owner and waits for its reply.
    pub(crate) async fn send(&self, command: ThreadCommand) -> Result<bool, Problem> {
        let (reply, replied) = CommandReply::channel();
        (self.0)(command, reply);
        await_reply(replied).await
    }
}

/// The review threads, as a page reaches them for its round's conversation.
#[derive(Clone)]
pub struct PageConversation {
    pub(crate) threads: ThreadsFeed,
    pub(crate) sender: ThreadSender,
}

impl PageConversation {
    pub fn new(threads: ThreadsFeed, sender: ThreadSender) -> Self {
        Self { threads, sender }
    }
}
