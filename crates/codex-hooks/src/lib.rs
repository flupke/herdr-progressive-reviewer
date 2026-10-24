//! The reviewer's Codex plugin, what its hook runs, and how `make install` installs it.
//!
//! The plugin (`plugin/`, listed by the marketplace of this directory) registers one
//! `UserPromptSubmit` hook, which runs `reviewer-control codex-hook` with Codex's event on its
//! standard input. Under Codex's app-server daemon, the hook runs in the daemon's process, with
//! the environment of whichever terminal started it, so it cannot tell the agent's pane nor its
//! Herdr server: it hands the prompt, with the agent's session, to every reviewer of the user
//! ([`agent_hooks::HookRoot`]), and a prompt a reviewer blocks does not run. With no reviewer
//! listening, it does nothing. [ADR 0010](../../../.agents/adr/0010-hear-codex-s-prompts-by-session.md)
//! says why.
//!
//! A Codex plugin's hook gets no option of the user's, and Codex runs it from a copy in its
//! home: [`install`] writes the plugin with the path of the checkout's `reviewer-control` in its
//! command, installs it with the `codex` CLI, and trusts its hook through Codex's app server, as
//! Codex's `/hooks` does, since Codex never runs an untrusted hook. [`uninstall`] removes the
//! trust, the plugin and its marketplace.

mod app_server;
mod hook;
mod install;

pub use hook::{SUBCOMMAND, run_hook};
pub use install::{install, uninstall};
