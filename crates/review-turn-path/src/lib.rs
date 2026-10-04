//! The path an Explore turn took after the reviewer's answer, when run-ahead watched the
//! question: the agent in the pane continued as the fork that prepared the turn, or took the
//! turn itself (the plain chain), and why. The run-ahead records keep it; the pane and the
//! Explore page show the reviewer one quiet line of it with the turn.

use serde::{Deserialize, Serialize};

/// The path of the turn after an answer.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "path", rename_all = "snake_case")]
pub enum TurnPath {
    /// The pane's agent continues as the fork `session`, whose turn is the round's.
    Prepared { session: String },
    /// The plain chain: the answer goes to the pane's agent, which takes the turn.
    Plain { reason: PlainReason },
}

/// Why an answer's turn could not be the turn a fork prepared: it would not be exactly the turn
/// the pane's agent takes for that answer, or the pane's agent could not continue as the fork.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum PlainReason {
    /// The answer has a comment.
    Comment,
    /// The answer is None of the above.
    NoneOfTheAbove,
    /// No fork was taken for the question: the agent was not idle yet, or forking failed, for
    /// `why` when known.
    NoForks { why: Option<String> },
    /// The setting did not fork the answer's choice.
    NotForked,
    /// The fork of the answer's choice has not submitted its turn yet.
    StillWorking,
    /// The fork of the answer's choice ended without a turn.
    NoTurn,
    /// The reviewer wrote in the round's conversation after the forks were taken, or wrote a
    /// message the agent has not answered yet: the forks know nothing of it.
    ChatMessage,
    /// The pane's agent was working.
    AgentBusy,
    /// The pane agent's input box held text.
    InputNotEmpty,
    /// The pane agent's session moved after the forks were taken.
    SessionMoved,
    /// The answer leaves other lines unreviewed than the fork was told.
    UnreviewedChanged,
    /// The agent's prompt for the answer differs from the fork's in more than its identities.
    PromptChanged,
    /// A check could not be made, for `error`.
    Unchecked { error: String },
    /// The pane's agent could not be switched to the fork's session, or the fork's turn could
    /// not be saved, for `error`.
    SwitchFailed { error: String },
    /// The reviewer stopped waiting, cancelled the answer or reset the round while the pane's
    /// agent switched to the fork's session: the fork's turn was not saved.
    Withdrawn,
}

impl TurnPath {
    /// The line the pane and the Explore page show with the turn, or `None` when the path says
    /// nothing the reviewer can use.
    pub fn line(&self) -> Option<&'static str> {
        match self {
            Self::Prepared { .. } => Some("Prepared while you were thinking"),
            Self::Plain { reason } => reason.line(),
        }
    }
}

impl PlainReason {
    /// Why no prepared turn was used, in plain words, or `None`. What the reviewer did, what
    /// the agent in the pane did and what changed in the review since the forks were taken
    /// come first; what the forks had not done, and the tool's own faults, after.
    fn line(&self) -> Option<&'static str> {
        Some(match self {
            Self::Comment => "Not prepared: you added a comment",
            Self::NoneOfTheAbove => "Not prepared: you answered None of the above",
            Self::ChatMessage => "Not prepared: you wrote in the chat",
            Self::AgentBusy => "Not prepared: the agent was working",
            Self::InputNotEmpty => "Not prepared: the agent's input box held text",
            Self::SessionMoved => "Not prepared: the agent's conversation moved on",
            Self::UnreviewedChanged => "Not prepared: the lines left to review changed",
            Self::PromptChanged => "Not prepared: the round changed after it was prepared",
            other => return other.not_ready_line(),
        })
    }

    /// The line of a reason that the forks had not done what the answer needs, or that the
    /// switch to a fork did not end with its turn, or `None` for a check that could not be made,
    /// which is in run-ahead's log. A failed or withdrawn switch leaves the turn waiting for
    /// Retry, which runs the plain chain: the turn it brings says why it took the agent's time.
    fn not_ready_line(&self) -> Option<&'static str> {
        match self {
            Self::NoForks { .. } => Some("Not prepared: nothing was prepared for this question"),
            Self::NotForked => Some("Not prepared: only the recommended choice is prepared"),
            Self::StillWorking => Some("Not prepared: the turn for this choice was not ready yet"),
            Self::NoTurn => Some("Not prepared: the turn for this choice did not finish"),
            Self::SwitchFailed { .. } => {
                Some("Not prepared: the agent could not switch to the prepared turn")
            }
            Self::Withdrawn => Some("Not prepared: you stopped waiting while it was being used"),
            // A check that could not be made, then the reasons whose lines are in `line`.
            Self::Unchecked { .. }
            | Self::Comment
            | Self::NoneOfTheAbove
            | Self::ChatMessage
            | Self::AgentBusy
            | Self::InputNotEmpty
            | Self::SessionMoved
            | Self::UnreviewedChanged
            | Self::PromptChanged => None,
        }
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
