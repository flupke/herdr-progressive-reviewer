//! Run-ahead with Claude Code: the forks of the session of a Claude Code agent in a Herdr pane.
//!
//! A fork is `claude -p --resume <session> --fork-session --session-id <new>` with the pane
//! agent's own binary, arguments, working directory and environment, less Herdr's variables,
//! so that it reads the agent's prompt cache and Herdr never takes it for the pane's agent. A
//! `PreToolUse` hook keeps it read-only: a hook leaves the tool list, and so the prompt cache,
//! as the agent has them. [`ClaudeForks`] is the [`review_run_ahead::ForkHost`] that starts,
//! follows and stops them.

mod arguments;
mod guard;
mod host;
mod pane;
mod stream;
mod transcripts;

pub use guard::{SUBMITS, run_guard};
pub use host::{ClaudeForks, ForkTools};
pub use transcripts::Transcripts;
