//! What the reviewer's front ends show of an implementation request, decided from its saved
//! dispatch state.

use serde::Serialize;
use ts_rs::TS;

use crate::DispatchState;

/// What became of an implementation request, as the front ends show it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum ImplementationState {
    /// The reviewer sends it to the agent now.
    Sending,
    /// The agent received it.
    Sent,
    /// It was saved, then not sent: the reviewer reopened first. The reviewer may send it, or
    /// a new one.
    Paused,
    /// Whether the agent received it is unknown. The reviewer may send a new one, after
    /// checking the agent's pane.
    Unknown,
    /// The agent did not start on it: the text may still wait in the agent's prompt box. The
    /// reviewer looks at the agent's pane, then sends it again as it is.
    NotStarted,
    /// It could not be sent, for this reason. The reviewer may send another.
    NotSent(String),
    /// The reviewer cancelled it before it was sent, and may send another.
    Cancelled,
}

impl ImplementationState {
    /// A request saved as `state`. One saved as queued or attempting is on its way when this
    /// process sends it (`sending`); otherwise an earlier process left it paused or with an
    /// unknown outcome.
    pub fn of(state: &DispatchState, sending: bool) -> Self {
        match state {
            DispatchState::Queued | DispatchState::Attempting if sending => Self::Sending,
            DispatchState::Queued => Self::Paused,
            DispatchState::Attempting | DispatchState::Unknown => Self::Unknown,
            DispatchState::Delivered => Self::Sent,
            DispatchState::NotStarted => Self::NotStarted,
            DispatchState::NotSent(reason) => Self::NotSent(reason.clone()),
            DispatchState::Cancelled => Self::Cancelled,
        }
    }

    /// Whether the reviewer may send another request in place of this one: the request is
    /// neither on its way nor received by the agent, and does not wait in the agent's prompt
    /// box, where a request the agent did not start on may still be.
    pub fn allows_another(&self) -> bool {
        matches!(
            self,
            Self::Paused | Self::Unknown | Self::NotSent(_) | Self::Cancelled
        )
    }
}
