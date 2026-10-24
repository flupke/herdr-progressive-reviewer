//! Run-ahead with Codex: the forks of the session of a Codex agent in a Herdr pane, run by the
//! agent's own app-server daemon.
//!
//! A fork is `thread/fork` of the agent's session on its daemon, then `turn/start` with the
//! prompt that follows the answer: the daemon runs its turn, with no new process, and its
//! requests start as the agent's, so that they read its prompt cache. The fork runs read-only:
//! the reviewer's Codex plugin's guard (`codex-hooks`), which the reviewer arms for the fork's
//! session, lets it read and submit and nothing else, and `thread/settings/update` gives its
//! session the read-only sandbox and an approval policy that asks before what the sandbox does
//! not cover. The reviewer is the fork's only client, so it answers each approval itself
//! (`approvals.rs`). A session keeps its settings once the agent resumes it: before the agent
//! does, the guard is lifted and the session takes back the agent's settings, which the fork's
//! answer gave.
//!
//! [`CodexForks`] is the [`review_run_ahead::ForkHost`] that starts, follows and discards them
//! (`turn/interrupt`, then `thread/delete`), and switches the agent to a fork's session with
//! Codex's own `/resume`, which the daemon confirms (`resume.rs`). The session of a Codex agent
//! is the one it named last in a call to the reviewer's tools, moved by the reviewer's own
//! switches, never Herdr's report: Herdr hears of a `/resume` only at the agent's next turn,
//! and under a shared daemon it can report another pane's session.

mod approvals;
mod fields;
mod host;
mod live;
mod resume;

pub use fields::{owns, turn};
pub use host::CodexForks;
