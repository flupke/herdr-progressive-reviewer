//! A client of the Codex app-server daemon, which follows the session of a Codex agent in a
//! Herdr pane: its turns, the prompt each one started on, its status, and its resumes.
//!
//! By default, the Codex TUI runs its sessions on an app-server daemon that every TUI of the
//! user's Codex home shares, and the daemon runs the hooks of every session with the
//! environment of the TUI that started it. So a hook cannot tell which pane it came from, and
//! Herdr's session for a Codex pane can be another pane's. The daemon itself tells every client
//! that joins a session what that session does: [ADR 0009](../../../.agents/adr/0009-hear-codex-through-its-app-server-daemon.md)
//! says why the reviewer hears Codex this way.
//!
//! [`Daemon::of_pane`] finds the daemon from the agent's process: its control socket in the
//! agent's Codex home, or the socket its `--remote` names. An agent started with `--no-daemon`
//! has none, and [`NoDaemon`] says why in words run-ahead can show. [`Daemon::follow`] joins
//! a session on a thread of its own and reports what it hears ([`SessionEvent`]); it connects
//! and joins again when the daemon restarts, as it does when it updates itself.
//!
//! [`Daemon::connect`] opens a connection of its own for anything else ([`Rpc`]): run-ahead
//! forks a session there, runs the fork's turn and answers its approvals.
//!
//! The daemon keeps no record of which client is on which session: the session of the agent
//! in a pane comes from the agent's own calls to the reviewer's tools, whose `_meta.threadId`
//! names it.

mod daemon;
mod follow;
mod rpc;
mod wire;

pub use daemon::{Daemon, NoDaemon};
pub use follow::{Follow, SessionEvent, SessionStatus, TurnEnd};
pub use rpc::{Incoming, Replier, Rpc};
