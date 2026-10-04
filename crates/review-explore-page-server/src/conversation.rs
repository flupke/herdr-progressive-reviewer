//! The review threads of a session, which hold its rounds' conversations, kept in memory as the
//! review tool's thread worker keeps them on disk: the page's thread commands change them, a
//! test plays the agent's replies and a wakeup that does not reach the agent, and every change
//! reaches the page.

use review_explore_page::{CommandRefusal, ThreadsFeed, ThreadsPublisher};
use review_threads::{AskedUnder, MessageId, Post, ReviewThreads, ThreadCommand, WakeupFailure};
use review_types::ReviewUnit;
use serde::Serialize;

use crate::sessions::Clock;

/// The review of every session's rounds.
const REVIEW: &str = "standalone-review";

/// The review threads of one session.
pub(crate) struct SessionThreads {
    threads: ReviewThreads,
    publisher: ThreadsPublisher,
    /// The messages the reviewer sent from the page, in order.
    sent: Vec<SentMessage>,
    /// The agent's replies so far, which name each reply.
    replies: usize,
    /// The clock that stamps each message the page shows.
    clock: Clock,
}

/// A message the reviewer sent from the page, as a test reads it back.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct SentMessage {
    round: String,
    text: String,
    asked_under: Option<AskedUnder>,
    quote: Option<String>,
}

impl SessionThreads {
    pub(crate) fn new(clock: Clock) -> Self {
        Self {
            threads: ReviewThreads::new(Self::review()),
            publisher: ThreadsPublisher::default(),
            sent: Vec::new(),
            replies: 0,
            clock,
        }
    }

    /// Hands the page the threads as they are now, each message stamped by the session's clock.
    fn publish(&self) {
        self.publisher.loaded(self.clock.stamped(&self.threads));
    }

    /// The review the session's rounds belong to.
    pub(crate) fn review() -> ReviewUnit {
        REVIEW.into()
    }

    pub(crate) fn subscribe(&self) -> ThreadsFeed {
        self.publisher.subscribe()
    }

    /// Carries out the page's thread `command`, as the review tool's thread worker would: a
    /// post wakes the agent, and Retry wakes it again.
    pub(crate) fn take(&mut self, command: ThreadCommand) -> Result<(), CommandRefusal> {
        let result = match command {
            ThreadCommand::Post { post, .. } => self.post(post),
            ThreadCommand::MarkRead {
                thread_id, through, ..
            } => self.threads.mark_read(&thread_id, through),
            ThreadCommand::Retry { thread_id, .. } => self.threads.retry(&thread_id).map(|()| {
                self.publisher.wakeup(Self::review(), None);
            }),
            _ => Ok(()),
        };
        self.publish();
        result.map_err(CommandRefusal::Failed)
    }

    fn post(&mut self, post: Post) -> Result<(), String> {
        let message = post.message().clone();
        let known = self.threads.message(&message.id).is_some();
        self.threads.post(post)?;
        if !known {
            let round = self
                .threads
                .thread_for_message(&message.id)
                .and_then(|thread| thread.round())
                .unwrap_or_default()
                .to_owned();
            self.sent.push(SentMessage {
                round,
                text: message.text,
                asked_under: message.asked_under,
                quote: message.quote,
            });
            // The wakeup is on its way.
            self.publisher.wakeup(Self::review(), None);
        }
        Ok(())
    }

    /// The agent replies `text` to the reviewer's latest message in the conversation of the round
    /// `round`. Returns false when that conversation has no message yet.
    pub(crate) fn agent_replies(&mut self, round: &str, text: &str) -> bool {
        let Some(thread) = self.threads.round_conversation(round) else {
            return false;
        };
        let Some(last) = thread.last_comment() else {
            return false;
        };
        self.replies += 1;
        let id = MessageId::parse(&format!("00000000-0000-4000-8000-{:012}", self.replies))
            .expect("a UUID");
        let post = Post::answer(thread.id.clone(), id, text.to_owned(), last.id.clone());
        if self.threads.answer(post).is_err() {
            return false;
        }
        self.publish();
        true
    }

    /// The wakeup for the reviewer's waiting messages did not reach the agent, for `error`.
    /// Returns false when no message waits.
    pub(crate) fn not_delivered(&mut self, error: &str) -> bool {
        let Some(through) = self.threads.pending_comment_sequence() else {
            return false;
        };
        let failure = WakeupFailure {
            through,
            error: error.to_owned(),
        };
        self.publisher.wakeup(Self::review(), Some(failure));
        true
    }

    /// The messages the reviewer sent from the page, in order.
    pub(crate) fn sent(&self) -> Vec<SentMessage> {
        self.sent.clone()
    }
}
