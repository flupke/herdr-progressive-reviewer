//! What the page tells the reviewer after an action it could not carry out: the socket's reply
//! to the action carries it, as a status card (`StatusCard`), which the page shows until the
//! round changes.

use crate::command::CommandRefusal;

/// An action that did not go through, and why. The page shows it as a status card
/// (`StatusCard`), which words it for the action.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Notice {
    pub(crate) action: Action,
    pub(crate) problem: Problem,
}

/// What the reviewer asked for with the action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Action {
    /// An answer to the question the page showed.
    Answer,
    /// The start of a round.
    Start,
    /// The reviewer's first pick of a blind question, before the recommendation shows.
    Pick,
    /// The implementation request of the conclusion the page showed.
    Implement,
    /// A pick of a quiz item, or a skip of the quiz, of the conclusion the page showed.
    Quiz,
    /// A reply to the conclusion the page showed.
    Reply,
    /// An action that recovers or closes the round.
    Recover(RecoveryAction),
}

/// An action of the page that recovers or closes the round, as the pane offers it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecoveryAction {
    /// Stop waiting for the start or the agent's turn the page showed.
    Stop,
    /// Retry of the agent's turn the page showed as interrupted.
    Retry,
    /// Cancel answer of the reviewer's latest answer.
    CancelAnswer,
    /// Reset of the round the page showed.
    Reset,
    /// Cancel of the implementation request the page showed as being sent.
    CancelImplementation,
}

impl RecoveryAction {
    fn name(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Retry => "retry",
            Self::CancelAnswer => "cancel-answer",
            Self::Reset => "reset",
            Self::CancelImplementation => "cancel-implementation",
        }
    }
}

/// Why an action did not go through.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Problem {
    /// The round moved on since the page was loaded.
    Stale,
    /// The round's owner could not carry out the action, for this reason.
    Failed(String),
    /// The round's owner did not reply: the action may have gone through.
    NoReply,
}

impl From<CommandRefusal> for Problem {
    fn from(refusal: CommandRefusal) -> Self {
        match refusal {
            // `CommandSender::send` answers a repeat as applied, not as a problem.
            CommandRefusal::Stale | CommandRefusal::AlreadyApplied => Self::Stale,
            CommandRefusal::Failed(reason) => Self::Failed(reason),
        }
    }
}

impl Action {
    /// The action's name, as the socket's replies say it.
    fn name(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Start => "start",
            Self::Pick => "pick",
            Self::Implement => "implement",
            Self::Quiz => "quiz",
            Self::Reply => "reply",
            Self::Recover(recovery) => recovery.name(),
        }
    }
}

impl Notice {
    /// The code of the socket's reply that carries the notice.
    pub(crate) fn code(&self) -> i32 {
        match self.problem {
            Problem::Stale => crate::rpc::RpcError::STALE,
            Problem::Failed(_) => crate::rpc::RpcError::FAILED,
            Problem::NoReply => crate::rpc::RpcError::NO_REPLY,
        }
    }

    /// What became of the action, in a few words, for the socket's reply: the page shows the
    /// notice's status card.
    pub(crate) fn message(&self) -> String {
        match &self.problem {
            Problem::Stale => format!("{} is stale", self.action.name()),
            Problem::Failed(reason) => reason.clone(),
            Problem::NoReply => "The review tool did not reply".to_owned(),
        }
    }

    pub(crate) fn new(action: Action, problem: Problem) -> Self {
        Self { action, problem }
    }
}
