# Report a Codex agent's resumed session to Herdr from the reviewer

When run-ahead switches a Codex agent to a fork's session, it types `/resume <session>` and the
daemon confirms that the TUI joined it ([ADR 0009](0009-hear-codex-through-its-app-server-daemon.md)).
Herdr hears of the new session only from its own Codex hook, which fires `SessionStart` with the
source `resume` at the agent's next turn. Until then, Herdr reports the agent on the session it
left. The pinned agent of the Explore session and the access identity of the thread service bind
the session Herdr reports, so they would refuse the agent once Herdr catches up, or accept the
next turn under the wrong session.

Once the daemon confirms a switch, or a move back, the Codex host reports the session to Herdr
itself, as Herdr's hook will at the next turn: `pane.report_agent_session` with the source
`herdr:codex`, `session_start_source: "resume"` and a `seq` in nanoseconds, which Herdr orders
among that source's reports. It does so only when Herdr reports a session for the pane, so an
agent whose Herdr hook does not run stays as Herdr sees it. The host also names the session as
the pane's for the reviewer's hooks (`AgentHooks::name_session`), which never take a fork's
session for the pane's.

Considered:
- Having the thread service's agent identity take a Codex agent's session from the reviewer,
  not from Herdr: a wider change, in a module every agent's prompts go through.
- Waiting for Herdr's hook at the agent's next turn: the next turn is the one the switch must
  send under the fork's session.
