# Progressive reviewer

https://github.com/user-attachments/assets/4f0c949d-3eb2-4dd4-bbcc-948ae47b0c41

Main features:

- Per-file turn-based reviews: send feedback to the LLM on a diff range, mark
  file as reviewed, see new diff since your last pass.
- LSP navigation.
- Full mouse support.
- Syntax highlighting.
- Vim movements.

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
