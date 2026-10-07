//! What the tool saves of the forks of a round, beside the round, so that a reopened reviewer
//! knows which forks existed, stops those that still run and deletes their transcripts.

use std::ops::Not;
use std::path::PathBuf;

use agent_fork::ProcessStamp;
use review_explore::InterviewUpdate;
use review_turn_path::TurnPath;
use serde::{Deserialize, Serialize};

use crate::answer::AnswerRecord;

/// How many forks of a round may fail in a row before run-ahead stops for the round.
pub const FAILURES_TO_HALT: u32 = 3;

/// Every fork run-ahead started in one Explore round, oldest first, and the path each answer
/// to a question it watched took.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct RoundForks {
    pub forks: Vec<ForkRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<AnswerRecord>,
    /// How many forks failed since the last one that kept a turn: each ended, or could not
    /// start, without a turn, before the tool discarded it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub failed_in_a_row: u32,
    /// When run-ahead stopped for the round, in milliseconds since the epoch, because
    /// [`FAILURES_TO_HALT`] forks failed in a row. No fork is taken for the round after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub halted_at_ms: Option<u64>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes a reference"
)]
fn is_zero(count: &u32) -> bool {
    *count == 0
}

impl RoundForks {
    /// The record of the fork `session`.
    pub fn fork(&self, session: &str) -> Option<&ForkRecord> {
        self.forks.iter().find(|fork| fork.session == session)
    }

    /// The record of the fork `session`.
    pub fn fork_mut(&mut self, session: &str) -> Option<&mut ForkRecord> {
        self.forks.iter_mut().find(|fork| fork.session == session)
    }

    /// The record of the answer that started the turn `request`.
    pub fn answer_mut(&mut self, request: &str) -> Option<&mut AnswerRecord> {
        self.answers
            .iter_mut()
            .find(|answer| answer.request == request)
    }

    /// Counts a fork that failed, at `at_ms`. Returns whether run-ahead stops for the round
    /// now, because [`FAILURES_TO_HALT`] forks failed in a row.
    pub fn fork_failed(&mut self, at_ms: u64) -> bool {
        self.failed_in_a_row += 1;
        if self.halted_at_ms.is_some() || self.failed_in_a_row < FAILURES_TO_HALT {
            return false;
        }
        self.halted_at_ms = Some(at_ms);
        true
    }

    /// A fork kept a turn: the forks that failed before it no longer count.
    pub fn fork_kept_a_turn(&mut self) {
        self.failed_in_a_row = 0;
    }

    /// The request of the answer that continued as the fork `session`, if one did.
    pub fn continued_as(&self, session: &str) -> Option<&str> {
        self.answers.iter().find_map(|answer| match &answer.path {
            TurnPath::Prepared { session: prepared } if prepared == session => {
                Some(answer.request.as_str())
            }
            _ => None,
        })
    }

    /// The ID under which the pane's agent knows the reviewer's answer `answer`: the answer
    /// its fork was told, when the agent continued as that fork, else `answer` itself.
    pub fn answer_as_told<'a>(&'a self, answer: &'a str) -> &'a str {
        self.answers
            .iter()
            .filter(|record| record.answer == answer)
            .find_map(|record| {
                let TurnPath::Prepared { session } = &record.path else {
                    return None;
                };
                self.forks
                    .iter()
                    .find(|fork| {
                        fork.session == *session
                            && matches!(fork.continued, Some(Continuation::Switched { .. }))
                    })?
                    .answer
                    .as_deref()
            })
            .unwrap_or(answer)
    }

    /// The path of each turn an answer started, by request: a prepared turn once the pane's
    /// agent runs its fork's session, and every plain chain. A turn whose switch to a fork did not
    /// end, after a reviewer that stopped meanwhile, has no path: Retry sends it to the agent.
    pub fn turn_paths(&self) -> impl Iterator<Item = (&str, &TurnPath)> {
        self.answers.iter().filter_map(|answer| {
            let shown = match &answer.path {
                TurnPath::Prepared { session } => self.forks.iter().any(|fork| {
                    fork.session == *session
                        && matches!(fork.continued, Some(Continuation::Switched { .. }))
                }),
                TurnPath::Plain { .. } => true,
            };
            shown.then_some((answer.request.as_str(), &answer.path))
        })
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
    /// The answer the fork was told, by ID: once the pane's agent continues as the fork, it
    /// knows the reviewer's answer by this ID. `None` in records saved before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// The session ID the fork runs as, chosen before it started.
    pub session: String,
    /// The session the fork was taken from: the pane's agent goes back to it when it may run
    /// the fork's session but the round did not take the fork's turn. `None` in records saved
    /// before it was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
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
    /// The switch of the pane's agent to the fork's session, once the reviewer's answer chose
    /// the fork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continued: Option<Continuation>,
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

    /// Whether the fork's session may be the pane agent's: its transcript is the agent's
    /// conversation then, and never deleted.
    pub fn is_agent_session(&self) -> bool {
        match self.continued {
            Some(Continuation::Switching { .. } | Continuation::Switched { .. }) => true,
            Some(Continuation::Failed { typed, .. }) => typed,
            Some(Continuation::Undone { .. }) | None => false,
        }
    }

    /// Whether the pane's agent may run the fork's session while the switch to it did not
    /// end: it runs, or ran in a reviewer that stopped, or it failed after the agent was told
    /// to resume the fork's session.
    pub fn is_unsettled(&self) -> bool {
        matches!(
            self.continued,
            Some(Continuation::Switching { .. } | Continuation::Failed { typed: true, .. })
        )
    }

    /// Whether the fork, ending now, failed: it kept no turn, and the tool did not discard it.
    pub fn ending_is_a_failure(&self) -> bool {
        self.turn.is_none() && self.discarded.is_none()
    }

    /// Whether a reviewer other than `reviewer`, which stopped, left the fork, which is not
    /// cleaned up yet, nor a session the pane's agent may run.
    pub fn left_by_a_stopped_reviewer(&self, reviewer: ProcessStamp) -> bool {
        !self.cleaned
            && !self.is_agent_session()
            && self.reviewer != reviewer
            && !self.reviewer.is_running()
    }

    /// Whether the fork runs for a reviewer other than `reviewer`, which still runs.
    pub fn runs_for_another_reviewer(&self, reviewer: ProcessStamp) -> bool {
        self.discarded.is_none()
            && !self.cleaned
            && self.reviewer != reviewer
            && self.reviewer.is_running()
    }

    /// Whether `reviewer` may settle the session of the fork: it started the fork, or the
    /// reviewer that did stopped.
    pub fn may_be_settled_by(&self, reviewer: ProcessStamp) -> bool {
        self.reviewer == reviewer || !self.reviewer.is_running()
    }

    /// Discards the fork for `reason`, at `at_ms`: its kept turn is dropped. A fork already
    /// discarded keeps its first reason, and a fork whose session may be the pane agent's is
    /// never discarded.
    pub fn discard(&mut self, reason: DiscardReason, at_ms: u64) {
        if self.discarded.is_some() || self.is_agent_session() {
            return;
        }
        self.discarded = Some(Discard {
            reason,
            prepared: self.turn.take().is_some(),
            at_ms,
        });
    }
}

/// Where the switch of the pane's agent to a fork's session stands.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Continuation {
    /// The switch started at `at_ms`, in milliseconds since the epoch.
    Switching { at_ms: u64 },
    /// Herdr reported the pane's agent on the fork's session at `at_ms`.
    Switched { at_ms: u64 },
    /// The switch failed at `at_ms`, for `error`. `typed` says whether the agent was told to
    /// resume the fork's session, which it may then run.
    Failed {
        at_ms: u64,
        error: String,
        typed: bool,
    },
    /// The switch did not hold: the round did not take the fork's turn, for `reason`, and at
    /// `at_ms` Herdr reported the pane's agent back on the session the fork was taken from.
    Undone { at_ms: u64, reason: String },
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
    /// The reviewer wrote in the round's conversation while the question waited: new forks
    /// replace them once the talk is quiet.
    ChatMessage,
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
    /// Too many forks of the round failed in a row: run-ahead stopped for the round.
    TooManyFailures,
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
