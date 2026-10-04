//! The browser tab's title, by the round's stage.

use serde::Serialize;

use super::steps::{Activity, Current, Position};

/// What the tab title says before " — <review>", so that a reviewer who left the tab sees whose
/// turn it is. The quiz belongs to the conclusion's stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabTitle {
    /// "Q3 · your turn": a question waits for the reviewer's answer.
    YourTurn { question: usize },
    /// "Agent working…": the agent works on its next turn.
    AgentWorking,
    /// "Retry needed": the agent is not working on the turn the round waits for, or the round
    /// has no turn to answer or send again and only Reset is left.
    RetryNeeded,
    /// "Conclusion": the round concluded, with its quiz.
    Conclusion,
}

impl TabTitle {
    pub(super) fn of(position: &Position<'_>) -> Self {
        match (position.activity, position.current) {
            (Activity::Working, _) => Self::AgentWorking,
            (Activity::Interrupted, _) | (Activity::Idle, Current::Design) => Self::RetryNeeded,
            (Activity::Idle, Current::Question(question)) => Self::YourTurn { question },
            (Activity::Idle, Current::Quiz | Current::Conclusion) => Self::Conclusion,
        }
    }
}
