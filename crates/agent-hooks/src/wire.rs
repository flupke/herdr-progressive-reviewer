//! What a hook sends a reviewer: one JSON line per connection.

use herdr_client::protocol::PaneId;
use serde::{Deserialize, Serialize};

/// An event of the agent of a Herdr pane, as its hook reports it.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Report {
    /// The pane of the agent.
    pub pane: PaneId,
    pub event: AgentEvent,
}

/// What the agent did.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEvent {
    /// The agent started the session `session`.
    SessionStarted {
        session: String,
        source: SessionSource,
    },
    /// The text `prompt` was submitted to the agent, in the session `session`: the agent takes a
    /// turn on it unless a reviewer blocks it. A command of Claude Code's own, such as
    /// `/resume`, is no prompt.
    PromptSubmitted { session: String, prompt: String },
}

/// A reviewer's answer to [`AgentEvent::PromptSubmitted`].
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct Answer {
    /// Why the reviewer blocks the prompt, as the agent shows it; `None` lets it go on.
    pub block: Option<String>,
}

/// Why the agent started a session, in Claude Code's words.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// The agent started.
    Startup,
    /// The agent resumed a session: with `/resume`, or started with `--resume`.
    Resume,
    /// `/clear` started a new session.
    Clear,
    /// The agent compacted its conversation.
    Compact,
    /// A source this reviewer does not know.
    #[serde(other)]
    Other,
}
