use serde::Serialize;

/// A wakeup for a review's pending comments that did not reach the agent. The comments it
/// covered stay pending until a reply takes them up; the reviewer may retry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WakeupFailure {
    /// The latest comment the wakeup covered, by its sequence: every pending comment up to
    /// it waits for a wakeup that reaches the agent.
    pub through: u64,
    /// Why the wakeup did not reach the agent.
    pub error: String,
}
