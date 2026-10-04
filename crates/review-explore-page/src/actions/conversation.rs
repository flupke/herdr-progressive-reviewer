//! The reviewer's actions in the round's conversation with the agent. The conversation is a
//! review thread: each action becomes the thread command the pane would send, for the owner of
//! the review threads, never a command of the round's owner, so that a message reaches the agent
//! as a thread's comment does and never answers the round's question. A message names the round
//! the page showed: one sent after that round was reset or replaced is refused as stale. A
//! message carries the identity the page chose for it, so that a repeat posts nothing.

use review_threads::{MessageId, Post, ThreadCommand};
use review_types::ReviewUnit;

use super::{Actions, crlf_to_lf};
use crate::Rounds;
use crate::conversation::{PageConversation, ThreadsSnapshot};
use crate::notice::{Action, Notice, Problem};
use crate::rpc::{ConversationCall, MessageParams, Outcome, ReadParams, RetryMessagesParams};

/// The conversation of the round a page showed, as the round and the review threads stand now.
struct Shown<'a> {
    unit: ReviewUnit,
    link: &'a PageConversation,
    threads: ThreadsSnapshot,
    /// Whether the reviewer can write: not in an earlier round.
    writable: bool,
}

impl<R: Rounds> Actions<'_, R> {
    pub(super) async fn in_conversation(&self, call: ConversationCall) -> Result<Outcome, Notice> {
        match call {
            ConversationCall::SendMessage(params) => self.send_message(params).await,
            ConversationCall::ReadMessages(params) => self.read_messages(params).await,
            ConversationCall::RetryMessages(params) => self.retry_messages(params).await,
        }
    }

    /// The conversation of the round `round`, while the page's round is that one.
    fn shown_conversation(&self, round: &str) -> Option<Shown<'_>> {
        let link = self.round.conversation.as_ref()?;
        let snapshot = self.round.stages.latest();
        let (shown, unit) = snapshot.conversation_round()?;
        if shown != round {
            return None;
        }
        Some(Shown {
            unit: unit.clone(),
            link,
            threads: link.threads.latest(),
            writable: !snapshot.earlier,
        })
    }

    /// Posts the reviewer's message in the conversation of the round the page showed. A repeat
    /// of a message the threads hold already changes nothing.
    async fn send_message(&self, params: MessageParams) -> Result<Outcome, Notice> {
        let failed = |reason: &str| Notice::new(Action::Message, Problem::Failed(reason.into()));
        let id = MessageId::parse(&params.id).map_err(|error| failed(&error))?;
        let text = crlf_to_lf(params.text);
        if text.trim().is_empty() {
            return Err(failed("A message cannot be empty"));
        }
        if self.posted(&id) {
            return Ok(Outcome::applied(false));
        }
        let shown = self
            .shown_conversation(&params.round)
            .filter(|shown| shown.writable)
            .ok_or_else(|| Notice::new(Action::Message, Problem::Stale))?;
        let quote = params.quote.filter(|quote| !quote.trim().is_empty());
        let post = Post::to_round(&params.round, text, params.asked_under, quote).with_id(id);
        let command = ThreadCommand::Post {
            review_unit: shown.unit,
            post,
        };
        send(shown.link, command, Action::Message).await
    }

    /// Whether the review threads of the page's round hold the message `id` already.
    fn posted(&self, id: &MessageId) -> bool {
        let Some(link) = self.round.conversation.as_ref() else {
            return false;
        };
        let snapshot = self.round.stages.latest();
        snapshot
            .review_unit
            .as_ref()
            .is_some_and(|unit| link.threads.latest().holds(unit, id))
    }

    /// Marks the agent's replies of the conversation read, through the position the view gave.
    async fn read_messages(&self, params: ReadParams) -> Result<Outcome, Notice> {
        let Some(shown) = self.shown_conversation(&params.round) else {
            return Ok(Outcome::default());
        };
        let Some(thread) = shown
            .threads
            .threads(&shown.unit)
            .and_then(|threads| threads.round_conversation(&params.round))
        else {
            return Ok(Outcome::default());
        };
        let command = ThreadCommand::MarkRead {
            review_unit: shown.unit.clone(),
            thread_id: thread.id.clone(),
            through: params.through,
        };
        send(shown.link, command, Action::Message).await
    }

    /// Wakes the agent again for the reviewer's messages that wait for a reply, as Retry agent
    /// does for a thread in the pane, unless none waits.
    async fn retry_messages(&self, params: RetryMessagesParams) -> Result<Outcome, Notice> {
        let stale = || Notice::new(Action::RetryMessages, Problem::Stale);
        let shown = self
            .shown_conversation(&params.round)
            .filter(|shown| shown.writable)
            .ok_or_else(stale)?;
        let thread = shown
            .threads
            .threads(&shown.unit)
            .and_then(|threads| {
                let thread = threads.round_conversation(&params.round)?;
                threads.retry(&thread.id).ok().map(|()| thread.id.clone())
            })
            .ok_or_else(stale)?;
        let command = ThreadCommand::Retry {
            review_unit: shown.unit.clone(),
            thread_id: thread,
        };
        send(shown.link, command, Action::RetryMessages).await
    }
}

/// Sends `command` to the owner of the review threads; `action` words the notice when it did not
/// go through.
async fn send(
    link: &PageConversation,
    command: ThreadCommand,
    action: Action,
) -> Result<Outcome, Notice> {
    match link.sender.send(command).await {
        Ok(applied) => Ok(Outcome::applied(applied)),
        Err(problem) => Err(Notice::new(action, problem)),
    }
}
