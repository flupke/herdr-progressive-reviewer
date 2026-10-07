# Hear the agent through the reviewer's Claude Code plugin

Run-ahead read Claude Code's input box off the agent's screen, and asked Herdr where the agent
stood until it reported the fork's session. A new design of that screen would break it, and
text left in the box could still meet the `/resume` the reviewer types. Claude Code's hooks say
exactly what the agent does: `SessionStart` names the session it resumed, and a
`UserPromptSubmit` hook sees the submitted text and can block it.

The reviewer ships a Claude Code plugin in this repository (`crates/claude-hooks`). `make
install` adds that directory as a local plugin marketplace and installs the plugin with the
path of the checkout's `reviewer-control` as its option; `claude plugin uninstall` removes it.
Herdr's own hook in the user's settings stays as it is. The plugin's hooks run in every Claude
Code session of the user, each as `reviewer-control agent-hook`, which does nothing outside a
Herdr pane or when no reviewer listens, and lets the agent go on when a reviewer does not answer
in time. Claude Code reads its hooks when it starts: an agent started before the install, or with
the plugin disabled, runs without them, and run-ahead is off for it, with a reason that says to
restart it, rather than falling back to the screen.

Each open reviewer listens on a Unix socket of its own in a directory per Herdr server,
`$XDG_RUNTIME_DIR/herdr-reviewer/<digest of HERDR_SOCKET_PATH>/`, which only the user can read.
A hook hands its event, with its pane (`HERDR_PANE_ID`), to every socket there, and each reviewer
keeps the events of the panes it expects one from.

Considered:
- Keeping the screen reads: they break with Claude Code's design, and cannot stop text left in
  the box.
- Writing the hooks into `~/.claude/settings.json`, as Herdr does: they mix with the user's own
  entries, and no single command removes them.
- The MCP bridge's HTTP port: it is named after a repository, which a hook would have to find
  from the agent's directory, while a pane belongs to a Herdr server; and a port on localhost is
  open to every local user, who could forge an event, where the runtime directory is the
  user's alone.
- A socket per pane, bound by the reviewer that drives it: the pane a reviewer drives changes
  while it runs, and it needs to know whether a pane's agent has the hooks before it listens to
  that pane.
