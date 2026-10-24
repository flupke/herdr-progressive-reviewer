<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo/logo-horizontal-dark.svg">
    <img alt="Progressive reviewer" src="docs/logo/logo-horizontal-light.svg" height="64">
  </picture>
</h1>

**Review the code your agent writes, without reading it twice.**

A code review pane for [Herdr](https://github.com/herdrdev/herdr), beside your coding agent.
Works with Git and jj, Claude Code and Codex.

## Progressive reviews

Mark files or hunks as reviewed. When the agent changes the code, only what changed since your
review comes back.

<img alt="The review pane: reviewed files checked, a diff that folds the reviewed hunks and shows the line changed since review, and the agent's reply in a review thread" src="docs/screenshots/pane-review.webp">

## Threads with your agent

Comment on any line, and your agent answers in the thread.

<img alt="The Threads tab: the review threads and their conversation with the agent" src="docs/screenshots/pane-threads.webp">

## Explore

The agent explains its change and asks you the decisions that matter, in your browser or on
your phone.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/screenshots/explore-question-dark.webp">
  <img alt="An Explore question on the Explore page, with its choices and the agent's recommendation" src="docs/screenshots/explore-question-light.webp">
</picture>

### Run-ahead (experimental)

While a question waits for you, run-ahead prepares the agent's next turn in the background:
it forks the agent's session for each choice its setting names, and each fork takes the turn
that would follow. When you pick a prepared choice, with no comment, the agent continues as that fork,
and the next question shows at once. A fork still working is waited for; Stop waiting hands
your answer to the agent.

Turn it on in the pane's Explore round settings (`z`): off, the recommended choice, or every
choice. It needs Linux, Claude Code, and the reviewer's Claude Code plugin that `make install`
installs: restart an agent started before the install.

Each fork costs about one agent turn, mostly read from the prompt cache, so every choice costs
more than the recommended one. Your answer goes to the agent as usual, and the forks are
discarded, when you add a comment, choose None of the above, send the agent back to the
drawing board or write in the round conversation, or when the agent or the review changed
since the forks started. A line under the turn says why. After three failures of its own in a
row, run-ahead stops for the round.

`bin/reviewer-control stats` says whether it pays: the share of answers that used a prepared
turn, why the others did not, your wait after an answer with and without one, and the tokens
the forks spent.

## Install

On Linux or macOS, install Herdr 0.7.5 or later, Rust, a C compiler, Make, Git, and Claude
Code's `claude` command, which `make install` runs. For jj repositories, use jj 0.43.0 or later.

```sh
git clone https://github.com/flupke/herdr-progressive-reviewer.git
cd herdr-progressive-reviewer
make install
```

Keep the checkout in place, and restart your agent to load the reviewer's tools.

## Get started

Add a shortcut to `~/.config/herdr/config.toml`, then run `herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "herdr.progressive-reviewer.toggle"
description = "toggle progressive reviewer"
```

Focus your agent's pane in the repository and press `prefix+d`. Press `?` for every shortcut.

Contributor instructions are in [AGENTS.md](AGENTS.md).
