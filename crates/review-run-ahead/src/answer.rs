//! What run-ahead did with each answer to a question it watched: the pane's agent continued as
//! the fork for the answer's choice, or took the turn itself, and why.

use serde::{Deserialize, Serialize};

/// One answer of the reviewer to a question run-ahead watched, and the path its turn took.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct AnswerRecord {
    /// The question answered, by ID and version.
    pub question: String,
    pub version: u32,
    /// The reviewer's answer, by ID.
    pub answer: String,
    /// The turn the answer started, by request.
    pub request: String,
    /// When the reviewer answered, in milliseconds since the epoch.
    pub at_ms: u64,
    pub path: TurnPath,
}

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
    /// The reviewer wrote in the round's conversation after the question.
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
