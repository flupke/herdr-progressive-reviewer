# Hear a Codex agent through its app-server daemon

Run-ahead has to know which session the pane's agent is on, when its turns start and end, and
whether a `/resume` the reviewer typed took effect. For Claude Code, the reviewer's plugin hears
this from the agent's hooks ([ADR 0005](0005-hear-the-agent-through-a-claude-code-plugin.md)).
The Codex TUI runs its sessions on an app-server daemon that every TUI of a Codex home shares,
and the daemon runs every session's hooks with the environment of the TUI that started it: a
hook's `HERDR_PANE_ID` can name another pane, and `SessionStart` for a resume fires only when
the next turn starts. Herdr's session for a Codex pane is wrong for the same reason when two
Codex panes share a daemon.

The reviewer joins the agent's session on the daemon instead (`crates/codex-daemon`): it
connects to the daemon's control socket, found from the agent's process, joins the session with
`thread/resume`, and hears its turns, their prompts and its status, as every client of the
session does. The session comes from the agent's own calls to the reviewer's tools, whose
`_meta.threadId` the MCP bridge forwards, since the daemon keeps no record of which client is on
which session. A resume is the goal snapshot the daemon sends to a session's clients on each
join, beyond the one the reviewer's own join causes. The client connects and joins again when
the daemon restarts, which it does when it updates itself. An agent started with `--no-daemon`
has no daemon: run-ahead will say so, rather than fall back to its hooks.

Considered:
- Codex's hooks, through a plugin like Claude Code's: under the daemon, they cannot tell which
  pane they come from, and `SessionStart` waits for the next turn, so a switch could not be
  confirmed.
- Herdr's session for the pane: it comes from the same hooks, and is wrong under a shared
  daemon.
- Asking the daemon which thread each client is on (`thread/loaded/list`): it lists the loaded
  threads, not who is on them, and idle threads stay loaded for minutes after their last client
  left.
- Supporting only `--no-daemon` agents, whose hooks run in the TUI with the pane's environment:
  the daemon is Codex's default, and a `--no-daemon` TUI has no server another client can join
  to fork or to watch.
