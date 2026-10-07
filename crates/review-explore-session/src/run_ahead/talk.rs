//! The reviewer's talk with the agent in the round conversation while a question waits: the
//! reviewer's messages the agent has yet to reply to, from those posted while it took the turn
//! that asks the question, and those that came since. A message changes what the agent knows, so forks taken before it cannot prepare the reviewer's next turn: they are
//! discarded as soon as the reviewer posts it. They are taken again only once the talk has been
//! quiet for a while, a minute in a reviewer: the agent replied to every message of the talk and
//! stayed idle, and the reviewer posted nothing new. A new message, or work of the agent, starts
//! the quiet wait again from the agent's next idle after its reply.

use std::sync::mpsc;
use std::time::Duration;

use review_run_ahead::DiscardReason;
use review_thread_service::RoundMessage;
use review_threads::MessageId;

use super::{Event, RunAheadInput, RunAheadState};
use crate::{ExploreSession, Input};

/// How long the talk stays quiet before the forks are taken again.
pub const TALK_QUIET: Duration = Duration::from_secs(60);

/// The reviewer's messages of a talk, and the quiet wait that runs once the agent replied.
#[derive(Default)]
pub(super) struct Talk {
    /// The reviewer's messages, oldest first.
    messages: Vec<MessageId>,
    quiet: Option<QuietWait>,
    /// Whether the log says that the idle agent has not replied yet, since the last message.
    unreplied_logged: bool,
}

/// A quiet wait that runs, by the number it was started under: dropping it stops it.
struct QuietWait {
    number: u64,
    _stop: mpsc::Sender<()>,
}

impl Talk {
    /// The talk of `messages`, which wait for the agent's reply, if any.
    pub(super) fn waiting_for(messages: Vec<MessageId>) -> Option<Self> {
        (!messages.is_empty()).then(|| Self {
            messages,
            ..Self::default()
        })
    }

    /// The agent works: the quiet wait, if one runs, stops.
    pub(super) fn interrupt(&mut self) {
        self.quiet = None;
    }
}

impl RunAheadState {
    /// The quiet wait of the talk about the question that waits, while one runs.
    fn quiet_wait(&self) -> Option<&QuietWait> {
        self.armed.as_ref()?.talk.as_ref()?.quiet.as_ref()
    }
}

impl ExploreSession {
    /// The watched agent is idle and the question that waits has no forks: they are taken, or,
    /// while the reviewer talks, the quiet wait starts once the agent replied.
    pub(super) fn run_ahead_idle(&mut self) {
        if self
            .run_ahead
            .armed
            .as_ref()
            .is_some_and(|armed| armed.talk.is_some())
        {
            self.talk_idle();
        } else {
            self.run_ahead_take();
        }
    }

    /// The reviewer posted `message`: in the round of the question that waits, its forks are
    /// discarded, and no fork is taken again until the talk has been quiet.
    pub(crate) fn run_ahead_round_message(&mut self, message: &RoundMessage) {
        let Some(armed) = self.run_ahead.armed.as_mut().filter(|armed| {
            armed.asked.round.unit == message.review_unit
                && armed.asked.round.instance == message.round
        }) else {
            return;
        };
        let talk = armed.talk.get_or_insert_default();
        talk.messages.push(message.message.clone());
        talk.quiet = None;
        talk.unreplied_logged = false;
        self.run_ahead.log(
            "the reviewer wrote in the round conversation: forks are taken again once it is quiet",
        );
        self.discard_taken(DiscardReason::ChatMessage);
    }

    /// The agent is idle while the reviewer talks: the quiet wait starts once the agent replied
    /// to every message of the talk.
    fn talk_idle(&mut self) {
        let Some(armed) = self.run_ahead.armed.as_ref() else {
            return;
        };
        let Some(talk) = armed.talk.as_ref().filter(|talk| talk.quiet.is_none()) else {
            return;
        };
        let round = &armed.asked.round;
        let replied = self
            .rounds
            .unanswered_round_messages(&round.unit, &round.instance)
            .map(|unanswered| {
                !unanswered
                    .iter()
                    .any(|(message, _)| talk.messages.contains(message))
            });
        match replied {
            Ok(true) => {}
            Ok(false) => {
                if let Some(talk) = self
                    .run_ahead
                    .armed
                    .as_mut()
                    .and_then(|armed| armed.talk.as_mut())
                    .filter(|talk| !talk.unreplied_logged)
                {
                    talk.unreplied_logged = true;
                    self.run_ahead.log(
                        "the agent is idle but has not replied in the round conversation: no \
                         forks until it does",
                    );
                }
                return;
            }
            Err(error) => {
                self.run_ahead.log(&format!(
                    "the agent's replies in the round conversation are unknown: {error}"
                ));
                return;
            }
        }
        self.run_ahead.quiet_waits += 1;
        let quiet = self.start_quiet_wait(self.run_ahead.quiet_waits);
        let wait = self.run_ahead.talk_quiet;
        self.run_ahead.log(&format!(
            "the agent replied in the round conversation: forks are taken once it is quiet for {}s",
            wait.as_secs()
        ));
        if let Some(talk) = self
            .run_ahead
            .armed
            .as_mut()
            .and_then(|armed| armed.talk.as_mut())
        {
            talk.quiet = Some(quiet);
        }
    }

    /// Starts the quiet wait `number`, on a thread of its own: once it passes, unless it was
    /// stopped, its end reaches the session's inbox.
    fn start_quiet_wait(&self, number: u64) -> QuietWait {
        let (stop, stopped) = mpsc::channel::<()>();
        let (inbox, wait) = (self.inbox.clone(), self.run_ahead.talk_quiet);
        std::thread::spawn(move || {
            if let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(wait) {
                inbox.deliver(Input::RunAhead(RunAheadInput(Event::Quiet { number })));
            }
        });
        QuietWait {
            number,
            _stop: stop,
        }
    }

    /// The quiet wait `number` passed: when it still runs, the talk is over and the forks are
    /// taken again.
    pub(super) fn run_ahead_quiet(&mut self, number: u64) {
        if self
            .run_ahead
            .quiet_wait()
            .is_none_or(|quiet| quiet.number != number)
        {
            return;
        }
        if let Some(armed) = self.run_ahead.armed.as_mut() {
            armed.talk = None;
        }
        self.run_ahead
            .log("the round conversation is quiet: taking the forks again");
        self.run_ahead_take();
    }

    /// Whether a quiet wait of the reviewer's talk runs.
    #[cfg(test)]
    pub(crate) fn quiet_wait_runs(&self) -> bool {
        self.run_ahead.quiet_wait().is_some()
    }

    /// The quiet wait that runs passes now.
    #[cfg(test)]
    pub(crate) fn end_quiet_wait(&mut self) {
        let number = self
            .run_ahead
            .quiet_wait()
            .expect("a quiet wait runs")
            .number;
        self.handle(Input::RunAhead(RunAheadInput(Event::Quiet { number })));
    }

    /// The reviewer's talk lasts `wait` of quiet, in place of a minute.
    #[cfg(test)]
    pub(crate) fn quiet_for(&mut self, wait: Duration) {
        self.run_ahead.talk_quiet = wait;
    }
}
