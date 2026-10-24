# Hear a Codex agent's prompts by its session, from every reviewer of the user

Herdr types the reviewer's prompts and commands into the agent's input box without emptying it,
so text left there joins them. For Claude Code, the `UserPromptSubmit` hook of the reviewer's
plugin confirms each typed prompt and blocks a joined one, and a hook reaches the reviewers of
its pane's Herdr server, by pane
([ADR 0005](0005-hear-the-agent-through-a-claude-code-plugin.md)). Codex's hooks can block a
prompt too, also under its app-server daemon, but the daemon runs every session's hooks with the
environment of whichever terminal started it
([ADR 0009](0009-hear-codex-through-its-app-server-daemon.md)): a hook's `HERDR_PANE_ID` and
`HERDR_SOCKET_PATH` can name another pane and another Herdr server, or be missing.

The reviewer's Codex plugin (`crates/codex-hooks`) registers `UserPromptSubmit` alone. Its hook
ignores the `HERDR_*` variables: it hands the prompt, with Codex's `session_id` (the thread ID),
to every reviewer socket under `$XDG_RUNTIME_DIR/herdr-reviewer/`, whatever the Herdr server,
and the reviewers of other servers let it go, since session IDs are unique. A reviewer keeps
its expectations of a Codex agent under the session the agent named in its last call to the
reviewer's tools (`_meta.threadId`), not under its pane. A session keeps the hooks it was loaded
with, so the hook records each session it runs in, under `herdr-reviewer/codex/`: a prompt to a
session with no record, or to an agent that named no session, goes through Herdr's status as
before. `make install` trusts the hook in the user's Codex configuration through Codex's app
server, as Codex's `/hooks` does, and `make uninstall` removes that trust with the plugin.

This extends ADR 0005: Claude Code's hooks still reach one server's directory, by pane.

Considered:
- Events by pane, as for Claude Code: under the daemon, the pane is the starting terminal's.
- Matching a reviewer's prompt by its text alone, without knowing the session: the reviewer
  could not tell a session without the hook from a slow one, and would wait on every prompt to
  an agent that runs none; nor block a joined `/resume`, whose session it must name.
- Herdr's session for the pane: Herdr hears it from its own hook, which the daemon runs with the
  same wrong environment.
- Forwarding `SessionStart`, `Stop` and `SessionEnd` too: the daemon reports turns, prompts and
  resumes (`crates/codex-daemon`), and `SessionStart` for a resume comes only with the next turn.
- Printing how to trust the hook with `/hooks` instead of trusting it: `make install` already
  writes the reviewer's registration into that configuration and installs the Claude Code
  plugin's hooks without review, and the hook does nothing when no reviewer listens.
