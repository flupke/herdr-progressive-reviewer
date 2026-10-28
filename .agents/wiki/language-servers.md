# Language servers

How the reviewer's language servers behave with direnv, which project root they get and why, how they hear of file changes, and their limits in Explore. The server commands and the TypeScript selection order are in `crates/review-lsp/src/language.rs` and `typescript_server.sh`.

## Environment

- When `direnv` is on the reviewer's `PATH`, a server starts through
  `direnv exec <project-root> <server> ...`. Otherwise it uses the inherited `PATH`.
- A direnv approval or setup failure is reported, with no fallback to the inherited
  environment.
- Startup allows up to five minutes for direnv and Nix to prepare dependencies.
- The reload command (`gR`) loads the environment again, restarts the active servers and
  reopens their documents.

## Project roots

- A project root is the outermost ancestor of the file, up to the repository root, that
  holds a root marker, so workspace members share one server.
- Expert builds and indexes its project after initialization; navigation results are
  empty until that work ends (it shows "Search index is loading").

## File changes

- The repository watcher sends the files changed since the last refresh to the running
  servers. The open documents among them are resynced with `didChange`, and a server that
  registered `workspace/didChangeWatchedFiles` hears the changes its globs match.
- The client does not offer `didChangeWatchedFiles`, so rust-analyzer keeps its own
  watcher; Expert registers its watchers (`**/*.{ex,exs}`, `**/mix.lock`) anyway.
- Expert reindexes a file only when an open document changes, as in an editor: a
  filesystem event only removes a deleted file from its index (0.1.11, and its main branch
  in October 2026). An Elixir file changed on disk while closed stays out of its index
  until it restarts, so a references list counts those files and says it may be
  incomplete; `gR` restarts Expert.

## Limits in Explore

- Evidence on the old side, and deleted lines, have no language-server operations: old
  coordinates are never sent to the live server. New-side navigation works.

## Tests

The tests with real language servers are opt-in ([Checks](checks.md), "Opt-in tests").

- Expert may not be on `PATH`: a Nix store build works, such as
  `PATH=$(ls -d /nix/store/*-expert-0.1.11)/bin:$PATH`, exported inside `scripts/dev-shell`,
  whose shell resets `PATH`.
- From a shell that direnv loaded, unset `DIRENV_DIFF`, `DIRENV_DIR`, `DIRENV_FILE` and
  `DIRENV_WATCHES` first: `direnv exec` otherwise unloads that environment, `PATH` with it,
  and the server fails with "command 'expert' not found".
