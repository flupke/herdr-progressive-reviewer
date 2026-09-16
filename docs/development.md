# Development

See the [README](../README.md#install) for building and installing the plugin.
Repository conventions and the feature workflow are in [AGENTS.md](../AGENTS.md).

## Checks

```sh
make check
```

In addition to the build dependencies, the checks use Herdr, Codex, Claude Code,
Python 3, `cargo-nextest`, `cccc`, and `jq`. Herdr integration tests run private
servers with isolated configuration, state and agent paths.

Optional real-language-server tests run in temporary projects:

```sh
cargo test -p review-lsp --test language_servers -- --ignored
```

See [language server setup](language-servers.md) for the server commands, and
[mutation testing](llm-mutation-testing.md) for mutation-test guidance.

## Diagnose UI stalls

Set `HERDR_REVIEWER_TIMINGS` to a JSONL file path when starting the reviewer.
It records event queue delays, handler times and frame render times.
