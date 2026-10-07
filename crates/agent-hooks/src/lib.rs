//! The events that the hooks of an agent in a Herdr pane send to the reviewers of its Herdr
//! server: run-ahead waits on them instead of reading the agent's screen.
//!
//! The agent runs the reviewer's hooks (`claude-hooks` holds Claude Code's plugin). Each hook
//! runs `reviewer-control agent-hook`, which hands the event, with the agent's pane, to every
//! reviewer of the agent's Herdr server ([`HookDirectory::tell`]). Each open reviewer listens on
//! a Unix socket of its own in the server's directory, under the user's runtime directory, and
//! follows the events of the panes it expects one from ([`AgentHooks`]). A hook outside Herdr,
//! or with no reviewer listening, does nothing; one that cannot reach a reviewer lets the agent
//! go on.

mod address;
mod directory;
mod listener;
mod wire;

pub use directory::HookDirectory;
pub use listener::{AgentHooks, Expectation, Heard};
pub use wire::{AgentEvent, Report, SessionSource};
