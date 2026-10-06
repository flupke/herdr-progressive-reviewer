# Language servers

How the reviewer's language servers behave with direnv, which project root they get and why, and their limits in Explore. The server commands and the TypeScript selection order are in `crates/review-lsp/src/language.rs` and `typescript_server.sh`.

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
- Expert builds and indexes its project after initialization; navigation results may be
  empty until that work ends.

## Limits in Explore

- Evidence on the old side, and deleted lines, have no language-server operations: old
  coordinates are never sent to the live server. New-side navigation works.

## Tests

The tests with real language servers are opt-in ([Checks](checks.md), "Opt-in tests").
