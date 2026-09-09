# Codex storage discovery after idle connections close

Investigated 2026-09-10. The installed CLI is 0.154.0; the two running TUIs
showing the failing descriptor pattern still execute 0.153.4. Their versions
were checked with their own executable's `--version`, not inferred from PATH.

## Concrete failure and minimal fix

Before the fix, `CodexProcess::sqlite_home` examined only the selected foreground
process's open files and recognized `state_N.sqlite` or
`thread_history_N.sqlite`. Zero matches caused the message “Cannot identify one
active Codex database directory; wait for its session to finish starting”.
This happens before the history reader or queue command starts.
[Resolver](../../crates/codex-comments/src/session.rs),
[connection setup](../../crates/codex-comments/src/lib.rs).

Read-only process metadata established the following pattern in running TUIs:

| Working-directory context | Open database families | Directories accepted by the old filter | Directories containing queue databases |
| --- | --- | --- | --- |
| `rayon/fuzzer` workspace | `logs_2`, `queue_1` | 0 | 1 |
| configuration workspace | `logs_2`, `queue_1` | 0 | 1 |
| this reviewer workspace, active research session | `state_5`, `thread_history_1`, `goals_1`, `logs_2`, `queue_1` | 1 | 1 |

Both failing-pattern processes were ordinary TUIs, not `app-server` processes
or explicit `--remote` clients. The screenshot's exact selected pane was not
identified, but the old resolver necessarily rejects both observed zero-match
cases. No database contents, prompts, transcripts, or configuration were read.

**Recognize `queue_N.sqlite` alongside the existing two filename families.**
Codex constructs the queue, state, and history paths from the same
`SqliteConfig::home()`, so an open queue file provides the same storage-directory
evidence. Preserve the exactly-one-directory check and strict numeric version
matching; do not fall back to an arbitrary directory or generic `.sqlite` file.
These definitions are identical in the affected 0.153.4 and installed 0.154.0.
[0.153.4 names/path construction](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/state/src/sqlite.rs#L29),
[0.154.0 queue/state/history getters](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/state/src/sqlite.rs#L129).

## Why the old assumption fails

Codex creates separate SQLite connection pools and sets their maximum size to
five without changing their idle or minimum-connection settings. Both versions
pin SQLx 0.9.0, whose defaults permit zero connections, close connections after
10 minutes idle, and impose a 30-minute maximum lifetime. A live runtime can
therefore retain its configured storage while no longer holding a state/history
database descriptor. The queue watcher polls every 10 seconds, explaining why
the queue file can remain open while other pools become idle.
[Codex pool configuration](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/state/src/sqlite.rs#L277),
[affected-version lockfile](https://github.com/openai/codex/blob/rust-v0.153.4/codex-rs/Cargo.lock#L13258),
[installed-version lockfile](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/Cargo.lock#L14358),
[SQLx defaults](https://docs.rs/crate/sqlx-core/0.9.0/source/src/pool/options.rs#143),
[queue watcher](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/ext/queue/src/service.rs#L92).

Idle-pool cleanup is a source-supported explanation for the observed descriptor
pattern; this investigation did not observe a connection's timed closure.
The missing queue-file recognition itself is directly established.

## Validation and scope

The existing isolated real-Codex E2E opens the connection immediately after
session discovery and supplies a private profile. It exercises a fresh storage
runtime and does not cover queue-only descriptors after an idle period.
Add a focused resolver regression with only an open `queue_1.sqlite`, plus
ambiguous-directory and invalid-filename cases. Retain the existing isolated
E2E for real queue submission and answer observation.
[Fixture](../../crates/codex-comments/tests/codex_fixture.py),
[E2E](../../crates/codex-comments/tests/native_queue.rs).

A broader daemon/configuration redesign is unnecessary for the observed cases.
`config/read` is not an active-storage query: it reloads configuration layers,
while environment/default SQLite-home resolution happens elsewhere, and a new
stdio app server initializes storage before handling the request. Codex 0.154.0
has no `debug config` command. An existing daemon can separately be reached via
`codex app-server proxy --sock PATH`, if that topology needs future support.
[Configuration reader](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/app-server/src/config_manager_service.rs#L115),
[SQLite-home resolution](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/core/src/config/mod.rs#L3976),
[startup initialization](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/app-server/src/lib.rs#L621),
[debug commands](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/cli/src/main.rs#L257),
[proxy](https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/cli/src/main.rs#L1410).

Research only: no implementation edits, compilation, tests, live submissions,
server/pane/configuration changes, or database-content reads were performed.
