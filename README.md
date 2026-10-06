<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo/logo-horizontal-dark.svg">
    <img alt="Progressive reviewer" src="docs/logo/logo-horizontal-light.svg" height="64">
  </picture>
</h1>

Review code with your coding agent inside Herdr, one file at a time.
Works with Git and jj repositories, Codex and Claude Code.

https://github.com/user-attachments/assets/4f0c949d-3eb2-4dd4-bbcc-948ae47b0c41

## Features

- Incremental reviews: mark files as reviewed and see only changes since your last pass.
- Persistent review threads with agent replies, available inline and in a dedicated Threads view.
- Experimental Explore interviews: answer policy questions beside working-copy code evidence.
- Syntax highlighting, search, and code navigation with language servers.
- Mouse support, Vim navigation, and Vim or regular comment editing.

## Install

On Linux or macOS, install Herdr 0.7.5 or later, Rust, a C compiler, Make and Git.
For jj repositories, use jj 0.43.0 or later. Agent features require Codex or Claude Code.

```sh
git clone https://github.com/flupke/herdr-progressive-reviewer.git
cd herdr-progressive-reviewer
make install
```

This builds and enables the Herdr plugin and configures both supported agents for
review comments. Keep the checkout in place after installation.
Start your agent after installing; an agent that is already running must reload
its MCP configuration, or restart, to load the reviewer's tools.

## Get started

To toggle the reviewer with `prefix+d`, add this to `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr.progressive-reviewer.toggle"
description = "toggle progressive reviewer"
```

Run `herdr server reload-config` in a terminal to load the shortcut. In Herdr,
focus an agent pane in the repository you want to review, then press `prefix+d`
(`Ctrl-b`, then `d` with the default prefix). The reviewer splits the focused
pane across the side that looks longer: beside a wide pane, below a narrow one.
The last focused agent in the workspace receives your comments and Explore
questions.

Select a file to inspect its diff. Select code to comment, then click **Post** or
press `Ctrl-Enter`. Replies appear in the same thread. Press `Space` to mark a
file as reviewed and `?` to see the keyboard shortcuts.

Contributor instructions are in [AGENTS.md](AGENTS.md) and the
[wiki](.agents/wiki/index.md) it points to.
