# Language servers

Which language server the reviewer starts for a file, in which environment, and for which project root. Language servers are optional: they give hover documentation, definitions, type definitions and references.

## Server commands

| Files | Server command |
| --- | --- |
| Rust (`.rs`) | `rust-analyzer` |
| Elixir (`.ex`, `.exs`, `.eex`, `.heex`) | `expert --stdio` |
| TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`) and JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`) | `tsgo --lsp --stdio`, TypeScript 7 `tsc --lsp --stdio`, or `typescript-language-server --stdio` |

For TypeScript and JavaScript, the nearest `node_modules/.bin` above the project root is
searched before `PATH`, and native servers come first: `tsgo`, then `tsc` from TypeScript 7
or later, then `typescript-language-server`. A later choice is used only when the earlier
ones are absent, not when they fail to start. The selection happens after the environment
below is loaded.

## Environment

- When `direnv` is on the reviewer's `PATH`, a server starts through
  `direnv exec <project-root> <server> ...`. Otherwise it uses the inherited `PATH`.
- A direnv approval or setup failure is reported, with no fallback to the inherited
  environment.
- Startup allows up to five minutes for direnv and Nix to prepare dependencies.
- The reload command (`gR`) loads the environment again, restarts the active servers and
  reopens their documents.

## Project roots

- The reviewer starts a server when a supported file is opened or queried. It finds the
  repository from the focused Herdr pane's working directory.
- A project root is the outermost ancestor of the file, up to the repository root, that
  holds `Cargo.toml`, `mix.exs`, or `tsconfig.json` / `jsconfig.json` / `package.json`, so
  workspace members share one server. A file with no such marker uses the repository root.
- A repository with several languages keeps independent servers.
- Expert builds and indexes its project after initialization; navigation results may be
  empty until that work ends.

## Limits in Explore

- Evidence on the old side, and deleted lines, have no language-server operations: old
  coordinates are never sent to the live server. New-side navigation works.
- A destination inside the repository opens on demand when it is a regular working-copy
  file. Destinations outside the repository and non-regular files stay unavailable.

## Tests

The tests with real language servers are opt-in ([Checks](checks.md), "Opt-in tests").
