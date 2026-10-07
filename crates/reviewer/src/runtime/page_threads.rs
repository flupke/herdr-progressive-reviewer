//! The Explore page's link to the review threads, which hold the round's conversation: the
//! thread worker's events publish the threads to the page, and the page's thread commands go to
//! the thread worker, the owner the pane's own commands go to. A post from the page gets its
//! reply once the worker says whether the threads took it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use review_explore_page::{
    CommandRefusal, CommandReply, ThreadSender, ThreadsFeed, ThreadsPublisher,
};
use review_thread_service::{self as comments, ThreadCommands};
use review_threads::{MessageId, ThreadCommand};

/// The review threads as the page sees them, and the page's posts that wait for the worker.
#[derive(Default)]
pub(super) struct PageThreads {
    publisher: ThreadsPublisher,
    /// The reply of each post the page sent that the worker has not reported yet.
    posts: Mutex<HashMap<MessageId, CommandReply>>,
}

impl PageThreads {
    pub(super) fn subscribe(&self) -> ThreadsFeed {
        self.publisher.subscribe()
    }

    /// Follows the thread worker's `event`: loaded threads and wakeups reach the page, and a
    /// post the page sent gets its reply.
    pub(super) fn observe(&self, event: &comments::Event) {
        match event {
            comments::Event::Loaded(loaded) => {
                if let Ok(threads) = &loaded.result {
                    self.publisher.loaded(threads.clone());
                }
            }
            comments::Event::Wakeup {
                review_unit,
                failure,
            } => self.publisher.wakeup(review_unit.clone(), failure.clone()),
            comments::Event::Posted(posted) => {
                if let Some(reply) = self.lock().remove(&posted.message_id) {
                    reply.send(posted.result.clone().map_err(CommandRefusal::Failed));
                }
            }
            comments::Event::RoundMessage(_) | comments::Event::Error(_) => {}
        }
    }

    /// Where the page sends its thread commands: to the worker behind `commands`.
    pub(super) fn sender(self: &Arc<Self>, commands: ThreadCommands) -> ThreadSender {
        let threads = Arc::clone(self);
        ThreadSender::new(move |command, reply| {
            threads.take(command, reply, |command| commands.send(command));
        })
    }

    /// Hands the page's `command` to the worker through `deliver`. A post waits for the worker's
    /// report; any other command is answered once handed over, as the worker reports its
    /// failures to the pane.
    fn take(
        &self,
        command: ThreadCommand,
        reply: CommandReply,
        deliver: impl FnOnce(ThreadCommand),
    ) {
        match &command {
            ThreadCommand::Post { post, .. } => {
                self.lock().insert(post.message().id.clone(), reply);
            }
            _ => reply.send(Ok(())),
        }
        deliver(command);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<MessageId, CommandReply>> {
        self.posts.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
#[path = "page_threads.tests.rs"]
mod tests;
