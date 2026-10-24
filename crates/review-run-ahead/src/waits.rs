//! How long every host waits for Herdr, the agent and its forks.

use std::time::Duration;

/// How long a host waits for Herdr, the agent and its forks. Production uses the defaults;
/// tests set them small.
#[derive(Clone, Copy, Debug)]
pub struct ForkWaits {
    /// How long a switch waits for the answer to the fork's submit to reach the fork, which then
    /// ends its turn, before it stops the fork anyway.
    pub answer: Duration,
    /// How long a switch waits, once the agent was told to resume the fork's session, until the
    /// host hears that the agent did and is ready for a prompt.
    pub resume: Duration,
    /// How long the agent's move back to the session its forks were taken from waits, as
    /// [`Self::resume`] does.
    pub settle: Duration,
    /// How often a switch asks where the agent stands, once it heard that the agent resumed
    /// the session: Herdr sends no event when an agent's session changes.
    pub resume_poll: Duration,
    /// How long a watch waits before it subscribes again after it lost what it watched.
    pub resubscribe: Duration,
}

impl Default for ForkWaits {
    fn default() -> Self {
        Self {
            answer: Duration::from_secs(10),
            resume: Duration::from_secs(20),
            settle: Duration::from_secs(20),
            resume_poll: Duration::from_millis(100),
            resubscribe: Duration::from_secs(1),
        }
    }
}
