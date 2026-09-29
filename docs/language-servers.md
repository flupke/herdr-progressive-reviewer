# Language servers

Language servers are optional and enable hover documentation, definitions, type
definitions and references. See the [README](../README.md) to install the reviewer.

Hover, definitions, type definitions, and references use these executables:

| Files | Server command |
| --- | --- |
| Rust (`.rs`) | `rust-analyzer` |
| Elixir (`.ex`, `.exs`, `.eex`, `.heex`) | `expert --stdio` |
| TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`) and JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`) | `tsgo --lsp --stdio`, TypeScript 7 `tsc --lsp --stdio`, or `typescript-language-server --stdio` |

Install [Expert](https://github.com/elixir-lang/expert/blob/main/pages/installation.md)
and [tsgo](https://github.com/microsoft/typescript-go) or
[typescript-language-server with TypeScript](https://github.com/typescript-language-server/typescript-language-server#installing)
separately or provide them through your project's direnv environment. When
`direnv` is available on the reviewer's `PATH`, servers start through
`direnv exec <project-root> <server> ...`. Otherwise, servers use the inherited
`PATH` directly. Direnv approval and setup failures are reported without
falling back to the inherited environment. Startup allows up to five minutes
for direnv and Nix to prepare dependencies. `gR` loads the environment again.
TypeScript server selection happens after loading that environment. The
nearest `node_modules/.bin` above the project root is searched before `PATH`,
and native servers come first: `tsgo`, then `tsc` from TypeScript 7 or later,
then `typescript-language-server`. A later choice is used only when earlier
ones are absent, not when they fail to start.

The reviewer starts each server when a supported file is opened or queried.
It discovers the repository from the focused Herdr pane's working directory.
Project roots come from `Cargo.toml`, `mix.exs`, or
`tsconfig.json` / `jsconfig.json` / `package.json` in the file's ancestors,
up to the repository root. The outermost matching directory is used so
workspace members share a server. Files without a matching marker use the
repository root.

Mixed-language repositories keep independent servers. `gR` restarts active
servers and reopens their documents. Expert builds and indexes its project
after initialization; navigation results may be empty until that work finishes.
