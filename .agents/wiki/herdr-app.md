# herdr-app and herdr-core

How herdr-app and herdr-core behave where the reviewer relies on Herdr. Checked against
herdr-app 0.1.12, herdr-core (`hc`) 0.1.17 and Claude Code 2.1.318. On this page, Herdr
means `herdr`, the older terminal multiplexer whose plugin the reviewer is; herdr-app calls
itself Herdr too, and labels its terminals `herdr.*`.

## Two programs, and where each API lives

- **herdr-core** (`hc`) runs every terminal and agent, and keeps them alive across window
  restarts. Its API is JSON-RPC 2.0 over `$HC_TERMINAL_SOCKET`, one message per line;
  `hc schema` prints it as OpenRPC. Terminals, their labels, their agent status, sends and
  events live here.
- **herdr-app** is the window: spaces, tabs, panes, layouts, keys, focus, and mods (GPUI
  panels in JavaScript). `herdr-app do <command>` runs a command; `herdr-app commands`
  lists them. Focus and zoom are per screen, so the core does not know which pane the user
  looks at (`~/.config/herdr-app/AGENTS.md`, "The model").
- herdr-app has no plugin system like Herdr's: no manifest, no `plugin link`, no build
  hook, no plugin state or config directory, no plugin ID, no `HERDR_*` variables in a pane.
  A terminal in herdr-app sees `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET` and `HC_SCOPE`, and
  carries the labels `herdr.space=<id>` and `herdr.tab=<id>` (`hc get $HC_TERMINAL_ID`).
- A mod reaches the window through `run()`, the live data (`focused()`, `agents()`,
  `terminals()`, `theme()`), and `exec`/`hc()` for programs. Mods are JavaScript and GPUI:
  no HTML, no web view, no Rust (`~/.config/herdr-app/AGENTS.md`, "What you can and
  can't do"). herdr-bowtie's mod `~/.config/herdr-app/mods/bowtie/main.js` starts its own
  program with `hc run`, passing `focused().tab` in its environment, and shows it in an
  overlay.
- `pane.split` takes `direction`, `pane`, `argv`, `mod` and `props`, but no folder;
  `terminal.open` takes `folder`.
- herdr-app writes `~/.config/herdr-app/view.json`: the selected space, the selected tab
  of each space, and the focused pane of each tab, by herdr-app's pane IDs.
- The `herdr-gpui-browser` skill is for Herdr GPUI, a different app; on Linux it opens
  pages in the system browser.

## Agents

- An agent is a terminal that reports its status with OSC 7501: `idle`, `working`, `done`,
  `blocked`, `error`, with an `app`. Claude Code reports `claude-code` by itself, with no
  hook. `herdr.d.ts` gives `codex` as an example `app`; whether Codex reports a status under
  hc is not verified. `turn` goes up each time the status leaves `working`.
- The core keeps no agent session ID. Herdr learned it from its own hooks,
  `~/.claude/hooks/herdr-agent-state.sh` and `~/.codex/herdr-agent-state.sh`, which exit at
  once without `HERDR_PANE_ID` and `HERDR_SOCKET_PATH`: in herdr-app nothing reports it.
- `terminal.send` pastes and presses Enter once nobody has typed for about 3 seconds. It
  refuses a `blocked` program unless `force`, types only on an exact status with `if`, and
  answers `started`: whether the program went `working` within a few seconds (null when it
  reports no status). With `wait`, it then waits for the end of that turn. `hc` wraps what
  one terminal sends another in `<hc from="...">` tags unless `notes = false` in
  `~/.config/herdr-core/config.toml`.
- `events.subscribe` streams every fact change of the terminals it matches (by label,
  terminal or parent): status, turn, title, labels, exit. A label change is an event;
  Herdr's `pane.report_agent_session` raised none.
- A terminal's `foreground` names only the leader of its foreground process group (pid,
  name, argv, cwd); Herdr's `pane.process_info` listed the processes.
- `hc --skill` says Codex must run with `--no-daemon` under hc, or its commands run outside
  hc. The reviewer hears Codex through that shared daemon (ADR 0009), so a Codex agent
  started the way hc says has no daemon to join.

## Herdr's APIs, and what herdr-app has in their place

`crates/herdr-client/src/protocol.rs` (`mod method`) and `rg 'HERDR_|herdr' crates tests
scripts nix Makefile` list the reviewer's uses.

| Herdr | In herdr-app and herdr-core |
|---|---|
| Plugin manifest, `herdr plugin link`, actions, the prefix+d binding (`herdr-plugin.toml`, `Makefile`, `README.md`, `scripts/package.sh`) | A mod, and a binding in `~/.config/herdr-app/keys.json`; no install command |
| `HERDR_PLUGIN_STATE_DIR`, `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_ID` | Nothing |
| `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID` | The `herdr.space` and `herdr.tab` labels of the review pane's terminal |
| `HERDR_PLUGIN_CONTEXT_JSON` (focused pane, its folder, tab) | A mod's `focused()`; nothing reaches a program unless the mod passes it |
| `session.snapshot`, `agent.list`, `agent.get` | `terminal.list` and `terminal.get`, filtered by label, with OSC 7501 status and `app` |
| The agent's session, `pane.report_agent_session` | Nothing |
| `pane.process_info` | `foreground`, the group leader only |
| `agent.prompt` | `terminal.send` |
| `agent.read` (tests only) | `terminal.read` |
| `events.subscribe`: `pane.focused` | Nothing in the core; a mod's `focused()`, and `view.json` |
| `events.subscribe`: `pane.agent_detected`, `pane.agent_status_changed` | `events.subscribe` status events |
| `plugin.pane.open`, `plugin.pane.focus`, `plugin.pane.close`, `agent.focus` | `pane.split` or `terminal.open`, `pane.focus`, `pane.close`, `agent.focus`, through `herdr-app do` or a mod's `run()` |
| `pane.get`, `pane.layout`, `pane.resize` | `herdr-app ls` and a mod's `panes()`, with each pane's tab; no pane size |
| `HERDR_PANE_ID`, `HERDR_SOCKET_PATH` in an agent's hooks | `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET` |
| An isolated `herdr server` in tests, the pinned Herdr in `nix/flake.nix`, agent-detection rules | A private core: its own `HC_SESSION` and scratch XDG paths (`hc --skill`); status comes from OSC 7501, not detection rules |
| The vision MCP's workspace in the user's Herdr | `herdr-app do space.new`, `pane.split`, `space.close`; `hc read`, `hc keys`, `hc send`; `herdr-app shot` for pixels |

The MCP bridge and the Explore page's browser use no Herdr API.

## The review pane outside Herdr

The reviewer needs nothing from Herdr to draw its pane and take keys, only Herdr's
variables: given stand-in values, it runs in a bare `hc` terminal. Without them it stops at
`HERDR_PLUGIN_STATE_DIR is not set`.
