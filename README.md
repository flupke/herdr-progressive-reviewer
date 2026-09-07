# Progressive reviewer

https://github.com/user-attachments/assets/4f0c949d-3eb2-4dd4-bbcc-948ae47b0c41

Main features:

- Per-file turn-based reviews: send feedback to the LLM on a diff range, mark
  file as reviewed, see new diff since your last pass.
- AI-generated inline review guides.
- LSP navigation.
- Full mouse support.
- Syntax highlighting.
- Vim movements.

## Language servers

Hover, definitions, and references use these executables:

| Files | Server command |
| --- | --- |
| Rust (`.rs`) | `rust-analyzer` |
| Elixir (`.ex`, `.exs`, `.eex`, `.heex`) | `expert --stdio` |
| TypeScript (`.ts`, `.tsx`, `.mts`, `.cts`) and JavaScript (`.js`, `.jsx`, `.mjs`, `.cjs`) | `typescript-language-server --stdio` |

Install [Expert](https://github.com/elixir-lang/expert/blob/main/pages/installation.md)
and [typescript-language-server with TypeScript](https://github.com/typescript-language-server/typescript-language-server#installing)
separately or provide them through your project's direnv environment. When
`direnv` is available on the reviewer's `PATH`, servers start through
`direnv exec <project-root> <server> ...`. Otherwise, servers use the inherited
`PATH` directly. Direnv approval and setup failures are reported without
falling back to the inherited environment. Startup allows up to five minutes
for direnv and Nix to prepare dependencies. `gR` loads the environment again.

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

Optional real-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
```

## Development install

Build both programs and link this directory:

```sh
make install
```

The Herdr action list then contains `open`, `close`, and `toggle`.

## Use

Example configuration, to put in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr.progressive-reviewer.toggle"
description = "toggle progressive reviewer"
```
