//! What the tool saves of the forks of a round, beside the round, so that a reopened reviewer
//! knows which forks existed, stops those that still run and deletes their transcripts.

use std::ops::Not;
use std::path::PathBuf;

use agent_fork::ProcessStamp;
use review_explore::InterviewUpdate;
use serde::{Deserialize, Serialize};

/// Every fork run-ahead started in one Explore round, oldest first.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct RoundForks {
    pub forks: Vec<ForkRecord>,
}

impl RoundForks {
    /// The record of the fork `session`.
    pub fn fork_mut(&mut self, session: &str) -> Option<&mut ForkRecord> {
        self.forks.iter_mut().find(|fork| fork.session == session)
    }
}

/// One fork, from before it starts until its process is gone and its transcript deleted.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ForkRecord {
    /// The question the fork answers, by ID and version.
    pub question: String,
    pub version: u32,
    /// The choice the fork was told the reviewer picked, by ID.
    pub choice: String,
    /// The session ID the fork runs as, chosen before it started.
    pub session: String,
    /// Where the agent keeps the fork's transcript.
    pub transcripts: PathBuf,
    /// The reviewer process that started the fork.
    pub reviewer: ProcessStamp,
    /// The fork's process, once it started.
    pub process: Option<ProcessStamp>,
    /// When the fork was taken, in milliseconds since the epoch.
    pub taken_at_ms: u64,
    /// The turn the fork submitted, which the tool keeps for its choice until the fork is
    /// discarded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<Box<InterviewUpdate>>,
    /// How its process ended, once it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<String>,
    /// The tokens of the fork's own requests, not of the history it started from, as far as
    /// it reported them before it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    /// Why and when the fork was discarded, once it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discarded: Option<Discard>,
    /// Whether its process is gone and its transcript deleted.
    #[serde(default, skip_serializing_if = "Not::not")]
    pub cleaned: bool,
}

impl ForkRecord {
    /// What stops the fork and removes what it left.
    pub fn trace(&self) -> crate::ForkTrace<'_> {
        crate::ForkTrace {
            session: &self.session,
            process: self.process,
            transcripts: &self.transcripts,
        }
    }

    /// Discards the fork for `reason`, at `at_ms`: its kept turn is dropped. A fork already
    /// discarded keeps its first reason.
    pub fn discard(&mut self, reason: DiscardReason, at_ms: u64) {
        if self.discarded.is_some() {
            return;
        }
        self.discarded = Some(Discard {
            reason,
            prepared: self.turn.take().is_some(),
            at_ms,
        });
    }
}

/// Why and when a fork was discarded.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Discard {
    pub reason: DiscardReason,
    /// Whether the fork had kept a turn.
    pub prepared: bool,
    /// In milliseconds since the epoch.
    pub at_ms: u64,
}

/// Why the tool discarded a fork.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DiscardReason {
    /// The reviewer answered its question.
    Answered,
    /// The reviewer cancelled the answer the question followed.
    AnswerCancelled,
    /// The question no longer waits, for another reason, such as another reviewer's answer.
    QuestionGone,
    /// The pane agent's session moved after the forks were taken: new forks replace them.
    SessionMoved,
    /// The reviewer turned run-ahead off.
    TurnedOff,
    /// The reviewer reset the round.
    Reset,
    /// The reviewer started a new round.
    NewRound,
    /// The reviewer showed another review.
    ReviewChanged,
    /// The reviewer closed.
    ReviewerClosed,
    /// The reviewer that started it stopped without discarding it; a later reviewer did.
    ReviewerStopped,
}

/// Tokens of one fork's own requests to the model.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TokenUsage {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl std::ops::AddAssign for TokenUsage {
    fn add_assign(&mut self, other: Self) {
        self.input += other.input;
        self.cache_creation += other.cache_creation;
        self.cache_read += other.cache_read;
        self.output += other.output;
    }
}

#[cfg(test)]
#[path = "record.tests.rs"]
mod tests;
