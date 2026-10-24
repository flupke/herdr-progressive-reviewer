//! What a hook sends a reviewer: one JSON line per connection.

use herdr_client::protocol::PaneId;
use serde::{Deserialize, Serialize};

/// An event of an agent, as its hook reports it.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Report {
    /// The pane of the agent, which Claude Code's hooks name. Codex's hooks cannot tell their
    /// pane, and name none: their event names the agent's session instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
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
    /// The text `prompt` was submitted to the agent, on the session `session`: the agent takes
    /// a turn on it unless a reviewer blocks it. A command of the agent's own, such as
    /// `/resume`, is no prompt. A Claude Code hook older than the session field reports none.
    PromptSubmitted {
        prompt: String,
        #[serde(default)]
        session: Option<String>,
    },
}

/// A reviewer's answer to [`AgentEvent::PromptSubmitted`].
#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Answer {
    /// Why the reviewer blocks the prompt, as the agent shows it; `None` lets it go on.
    pub(crate) block: Option<String>,
}

/// Why the agent started a session, in Claude Code's words.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionSource {
    /// The agent resumed a session: with `/resume`, or started with `--resume`.
    Resume,
    /// Another source: the agent started (`startup`), `/clear` started a new session, or the
    /// agent compacted its conversation (`compact`).
    #[default]
    #[serde(other)]
    Other,
}
