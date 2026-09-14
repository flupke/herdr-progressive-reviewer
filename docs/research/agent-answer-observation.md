# Observing agent answers

Checked 2026-09-10 against official documentation, Codex `rust-v0.153.4`,
Anthropic SDK source, installed Herdr 0.8.2's offline schema, and Herdr's public
`v0.9.0` source. The reviewer now uses the Codex `thread/read` approach described
below for new submissions. Compatibility experiments used isolated real Herdr
and Codex processes with a deterministic local provider and private storage.

| Question | Finding |
| --- | --- |
| Is Codex's local transcript schema stable? | No explicit compatibility contract; its hooks documentation explicitly calls the transcript format unstable. |
| Can we reuse Codex Rust crates? | Yes, through a pinned upstream checkout, but they bring a substantial dependency graph and do not make the format stable. |
| What about Claude? | Its transcript is also internal. Official SDK history readers and hooks offer better boundaries; native queued inputs can share an answer. |
| Does Herdr already expose answers? | Neither the installed 0.8.2 integration nor public 0.9.0 exposes structured answer events or a correlated answer-read API. |

The evidence and qualifications for each finding follow.

## Codex: typed internal data is not a stable interface

The official hooks reference explicitly warns that the transcript format can
change. A documented `transcript_path` locates a file; it does not stabilize its
contents. [Common hook inputs](https://learn.chatgpt.com/docs/hooks#common-input-fields).

At the inspected tag, upstream `codex-history` defines `RolloutLine` and
`RolloutItem`; `codex-protocol` defines the embedded events. `TurnComplete` uses
the wire name `task_complete`, accepts `turn_complete`, and includes `turn_id`,
optional `last_agent_message`, and optional `error`. The tagged enums lack an
unknown-event catch-all, so importing their types does not automatically tolerate
new event variants. [History types](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/history/src/lib.rs),
[wire enum](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/history/src/rollout_payload.rs),
[protocol events](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/protocol/src/protocol.rs).

Upstream `codex-rollout` exposes `decode_rollout_line(serde_json::Value)`,
`open_rollout_line_reader`, and `RolloutRecorder::load_rollout_items`. The decoder
handles an upstream Serde flattening/number-representation pitfall. The reader
supports upstream compression handling; the batch loader skips and counts bad
records. These are useful reuse points, but none supplies our comment-to-turn
correlation state machine. [Decoder exports](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/lib.rs),
[record reader](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/compression.rs),
[batch loader](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/src/recorder.rs).

These Apache-2.0 workspace crates are technically reusable with a pinned Git
dependency or vendoring. They are not a small standalone reader: history pulls
protocol and internal networking/image dependencies; rollout adds state/SQLite,
telemetry, compression, and other workspace crates. The upstream toolchain is
1.95.0. Compatibility with this workspace's Rust 1.89 was not built or tested;
the toolchain declaration alone does not establish an MSRV. [Workspace](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/Cargo.toml),
[history manifest](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/history/Cargo.toml),
[protocol manifest](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/protocol/Cargo.toml),
[rollout manifest](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rollout/Cargo.toml),
[toolchain](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/rust-toolchain.toml).

Registry checks found no published `codex-history` or `codex-rollout`. The
published `codex-protocol` 0.63.0 archive identifies Namastex Labs' fork, not the
current official crate. Selecting that dependency by name would be misleading.
[Registry entry](https://index.crates.io/co/de/codex-protocol),
[published archive](https://static.crates.io/crates/codex-protocol/codex-protocol-0.63.0.crate).

## Codex: structured observation options

The app-server API documents `thread/read` with `includeTurns: true` for reading
stored history without resuming the thread. This is the implemented recovery path:
Codex owns the file decoding and projects structured turns/items. A read does
not subscribe to another owner's live events. The owning app-server can emit
`turn/completed` with completed/interrupted/failed status. Pagination endpoints
are experimental; the documentation also labels the app-server command and
WebSocket transport experimental, despite distinguishing stable and experimental
API methods. [App-server documentation](https://learn.chatgpt.com/docs/app-server).

The reviewer requires the exact submitted user text, the native turn's
`completed` status with a non-null `completedAt`, and one `final_answer` ending
with `[review-answer: ID]` on its own line. The ID is unique per submission
attempt; the displayed reply omits that line. A separate app-server normalizes
another owner's unfinished turn to `interrupted`, so that status alone cannot
settle delivery. A recorded interruption with `completedAt` does mark the comment
failed so it can be retried. Missing or ambiguous markers become uncertain observations.
[Read normalization](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/app-server/src/request_processors/thread_processor.rs#L5640),
[completion projection](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/app-server-protocol/src/protocol/thread_history_projection.rs#L20).

The reader uses the selected owner's executable, homes, working directory and
effective SQLite directory. App-server rejects `--profile`; on Linux, the
reviewer identifies the unique directory of that owner's open `state_*.sqlite`
and `thread_history_*.sqlite` descriptors and pins it explicitly. This lookup and
the child-only remote-control disable flag are version-specific implementation
details. The RPC client only initializes and reads; app-server startup can still
initialize databases and logs. Queue submissions retain the owner's profile.
[CLI profile routing](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/cli/src/main.rs#L1824),
[SQLite names](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/state/src/sqlite.rs#L29),
[remote-control switch](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/app-server-transport/src/transport/remote_control/mod.rs#L87).

One persistent reader shares snapshots across pending comments, at most every
500 ms per session. In 0.153.4, full `includeTurns` hydration supports both legacy
and paginated histories, but is deprecated for paginated storage. The client
accepts interleaved deprecation notices, limits responses to 64 MiB and requests
to five seconds, and reports read errors without resending. Persisted projections
can lag canonical history after a database error; polling has no guaranteed
completion deadline. Old receipts still use their recorded rollout baseline.
[Full-read compatibility](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/app-server/src/request_processors/thread_processor.rs#L3281),
[projection ordering](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/thread-store/src/local/live_writer.rs#L308).

The native queue E2E test verifies two separate replies, the unfinished composer
draft, recovery while a turn is pending, and a profile-specific SQLite directory.
Protocol tests cover notifications, read-only method selection, malformed data,
wrong-thread replies, API errors, timeouts and subprocess cleanup.

Native hooks provide another boundary: `UserPromptSubmit` includes `prompt` and
`turn_id`; `Stop` includes that turn ID and optional `last_assistant_message`.
Stop runs before all continuation decisions are settled, so an observation there
is a candidate answer. Hooks can block input or request more work.
[Hook contracts](https://learn.chatgpt.com/docs/hooks).

Codex's documented legacy `notify` callback receives `agent-turn-complete` JSON
with `thread-id`, `turn-id`, and `last-assistant-message`. Source inspection shows
it runs after Stop-hook continuation decisions. It spawns the callback without
waiting for an acknowledgment or retrying delivery; it is not a durable receipt.
The source also anticipates eventual removal of this legacy mechanism.
[Notification configuration](https://learn.chatgpt.com/docs/config-file/config-advanced#notifications),
[callback implementation](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/hooks/src/legacy_notify.rs),
[Stop/notify ordering](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/session/turn.rs).

Crucial correlation detail: notify's `input-messages` comes from user messages
throughout the final model prompt history, not just the initiating comment.
Searching it for any review marker could repeatedly attribute later answers to
older comments. Native queue input does pass through `UserPromptSubmit`, which
sees the pending text and the same turn ID used by Stop/notify. Use that to
establish a provisional exact mapping, then confirm acceptance and completion.
Steering can add multiple inputs to one turn. [Queue dispatch](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/ext/queue/src/service.rs),
[hook input construction](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/hook_runtime.rs),
[input acceptance](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/session/turn.rs).

This requires configuring the actual owner session. Legacy notify settings stay
session-static during reload, whereas layer-backed hooks can refresh; changed
hooks require trust. Configuring a queue-sender subprocess does not retrofit the
running owner's callbacks. [Reload behavior](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/core/src/session/mod.rs),
[hook trust](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks).

## Claude: supported readers and hooks, with grouped inputs

Claude explicitly describes its JSONL format as internal and changeable. Its
first-party SDKs expose read-only `getSessionMessages()` in TypeScript and
`get_session_messages()` in Python. They are useful for history access without
starting a model query. [Transcript contract](https://code.claude.com/docs/en/sessions#export-and-locate-session-data),
[SDK history guide](https://code.claude.com/docs/en/agent-sdk/sessions).

The Python reader reconstructs the visible conversation through parent links and
filters internal/sidechain entries. Its public messages have role, UUID, session
ID, and content, but no prompt ID or turn-completion receipt. Compaction can
replace earlier context with a summary. This is a snapshot reader, not a lossless
event subscription. [Reader API](https://code.claude.com/docs/en/agent-sdk/python#get_session_messages),
[reader implementation](https://github.com/anthropics/claude-agent-sdk-python/blob/main/src/claude_agent_sdk/_internal/sessions.py).

Current hooks include `prompt_id` in common input, added in Claude Code 2.1.196.
`UserPromptSubmit` supplies prompt text; `Stop.last_assistant_message` supplies
the final response text directly. Stop can precede a hook-requested continuation
and excludes interrupts; `StopFailure` reports failures separately. `MessageDisplay`
has message-level boundaries, not completion proof. [Hook reference](https://code.claude.com/docs/en/hooks).

For an ordinary prompt starting a turn, mapping its review marker to
`(session_id, prompt_id)` and then observing Stop is a plausible design. It is
not sufficient for arbitrary queued comments: Claude documents that queued plain
messages can enter the active turn after tool calls finish. Several comments can
therefore receive one combined answer. The docs do not promise an independent
submit/Stop pair for every such pickup. We need either grouped-answer semantics
or dispatching one comment per idle boundary. [Interactive queue semantics](https://code.claude.com/docs/en/interactive-mode#queue-messages-while-claude-works).

When an application owns execution through the SDK, supplied input UUIDs offer
stronger correlation. Current TypeScript results can echo `user_message_uuids`
for all answered inputs, up to 64; the singular field only identifies the last
merged input. This still identifies an answer group, not separate answers. The
SDK normally starts its own CLI subprocess; resume continues execution and is
not a read-only subscription to an existing terminal. [SDK correlation](https://code.claude.com/docs/en/agent-sdk/typescript#user_message_uuids),
[execution model](https://code.claude.com/docs/en/agent-sdk/hosting#the-subprocess-model).

## Herdr: session reporting exists; answer reporting does not

The installed 0.8.2 offline schema, protocol 20, supplies agent/session identity,
lifecycle state, prompt dispatch, and terminal reads. It has no structured
answer event, transcript/messages endpoint, or native turn receipt. `agent prompt
--wait` does not track individual turns: an already-active turn finishing can
satisfy the wait. [Agent schema](https://github.com/herdrdev/herdr/blob/v0.8.2/src/api/schema/agents.rs),
[event schema](https://github.com/herdrdev/herdr/blob/v0.8.2/src/api/schema/events.rs),
[automation semantics](https://herdr.dev/docs/agent-automation/#choose-the-control-surface).

Static inspection found installed Codex hook version 6 and Claude hook version 7,
both registered only for SessionStart and forwarding session identity. The public
0.9.0 integrations remain session-only: Codex hook version 8 and Claude version 9
send `pane.report_agent_session`, not answer text or turn IDs. The 0.9.0 schema,
protocol 22, still lacks the answer capability, so upgrading alone does not solve
this. [0.9.0 schema](https://github.com/herdrdev/herdr/blob/v0.9.0/docs/next/api/herdr-api.schema.json),
[Codex hook](https://github.com/herdrdev/herdr/blob/v0.9.0/src/integration/assets/codex/herdr-agent-state.sh),
[Claude hook](https://github.com/herdrdev/herdr/blob/v0.9.0/src/integration/assets/claude/herdr-agent-state.sh).

A Herdr plugin can subscribe to supported Herdr events through
`HERDR_PLUGIN_EVENT_JSON`. That envelope does not contain the original native
agent hook payload. Supported event names are a closed list, with no answer
event or arbitrary event-publish API. A plugin could own a separate native hook
collector, or Herdr could gain a new answer-report request and event. Those are
new implementations. Keep custom native hooks in separate files because Herdr
overwrites its managed scripts during integration updates. [Plugin contract](https://herdr.dev/docs/plugins/),
[supported events](https://github.com/herdrdev/herdr/blob/v0.9.0/src/api/schema/events.rs),
[managed script guidance](https://github.com/herdrdev/herdr/blob/v0.9.0/src/integration/assets/codex/herdr-agent-state.sh).

## Recommendation for the reviewer

Prefer an agent-specific observer behind a shared answer model. For Codex,
use `thread/read` for stored-history reconciliation with exact input correlation
and the final answer marker. Native hooks remain an alternative for live capture. Importing
the upstream Rust graph solely for final text seems disproportionate and would
still require compatibility maintenance. For Claude, use first-party readers
and hooks while explicitly supporting grouped inputs or serial dispatch.

The architectural inference is that Herdr's agent integration is a natural home
for normalizing these product-specific contracts, but Herdr does not currently
provide that service. A plugin collector could be an intermediate implementation.
Track native session and input/turn IDs, accepted inputs, candidate output,
verified completion, interruption/failure, and missing observations explicitly;
persist and deduplicate observations. Any future hook collector also needs
coverage of continuation and missed callbacks.
