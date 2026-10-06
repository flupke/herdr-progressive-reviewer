# Explore settings

The settings of Explore rounds and of the Explore page: what each one does, when a change applies, and where they are saved.

## Where they live

- Every screen of the pane's Explore tab shows the round settings, then the page settings,
  above the page's address and QR code. Each setting has a key, which works while the
  reviewer is not typing an answer.
- They are saved with the reviewer's other settings, in `settings.json` in
  `$HERDR_PLUGIN_STATE_DIR`, which the reviewers of the machine share
  (`ExploreRoundSettings` in
  [`crates/review-explore-round-settings`](../../crates/review-explore-round-settings),
  `ExplorePageSettings` in
  [`crates/review-explore-page-settings`](../../crates/review-explore-page-settings)).
- A change saves only the setting it changes, so it keeps what another reviewer saved, and
  the pane then shows the settings as saved.
- No setting needs the reviewer to be reopened. Another reviewer that is already open does
  not see a change at once: it keeps its own settings, and network access as it was, until
  it is reopened or saves a change of its own; then it applies the saved settings.

## Round settings

| Setting | Key | Effect |
| --- | --- | --- |
| Writing style of the next round | `W` | Simplified Technical English, the default: the prompts ask the agent to apply the writing rules of ASD-STE100, without its controlled dictionary, to every text the reviewer reads, on every turn, the conclusion and the quiz included. Plain: the agent writes in its own style. The style changes the wording and the length, not what the agent says. |
| Run ahead (experimental) | `z` | Off, the default; the recommended choice; or every choice. See [Run-ahead](run-ahead.md). |

- A round keeps the writing style it started with: a change applies to the next round. A
  round saved before the writing style existed continues in the plain style.
- Run-ahead applies to the question that waits at the reviewer's next action: turning it
  off then stops the forks that run.

## What run-ahead does for the reviewer

Run-ahead is experimental: the turns it prepares can be wasted when the reviewer talks with
the agent in the chat while a question waits, and its behaviour may change.

- While a question waits, the review tool forks the session of the agent in the pane once
  per choice the setting names, as background `claude` processes, once the agent is idle.
  Each fork takes the turn that would follow that answer, and the review tool keeps what
  it submits.
- An answer with a choice whose fork submitted its turn, with no comment, and with nothing
  changed since the forks were taken, makes the agent in the pane resume the fork's
  session: the fork's turn becomes the round's, and its question shows within about a
  second, with the line "Prepared while you were thinking" in the pane and on the page.
- Any other answer goes to the agent in the pane, and its turn says why in one quiet line,
  such as "Not prepared: you added a comment".
- A talk with the agent while the question waits, in the chat or in its pane, moves its
  session: once the agent is idle again, the forks are taken again from the session as the
  talk left it.
- When the agent cannot switch, the turn waits for Retry, which sends the answer to the
  agent in the pane.
- The other forks are stopped and their transcripts deleted when the reviewer answers,
  cancels an answer, resets, or closes the reviewer.
- Each fork costs about one agent turn; it reads the agent's prompt cache.
- It needs Claude Code in the agent's pane, started with options run-ahead knows and
  without a `--settings` of its own. It works on Linux only.
- What the forks do is written to `run-ahead.log` in the plugin's state directory.

## Page settings

| Setting | Key | Effect |
| --- | --- | --- |
| Open the page on Start | `w` | On, the default: Start and Start with Challenger open the round's page in the browser. Off: they start the round in the pane. |
| Serve on the network | `n` | On, the default: the page is also served to the network, and the pane shows its address and QR code. Off: the page stays on this machine; the network listener stops, a phone's page stops working, and the address and QR code leave the pane. |
| Interface | `N` | The interface whose IPv4 address the page listens on, such as `wlan0` or a VPN's `tailscale0`. Empty, the default (shown as "default route"): the interface of the route to the internet. |
| First port | `#` | The first port tried, 8790 by default. When another reviewer holds it, the page takes the next free one of the ten ports from it. |
| Share over a tunnel | `O` | Off when the reviewer starts, and never saved. On: shares the running round over a Cloudflare quick tunnel; it goes off with the round. |

- The three switches turn over at once. Interface and First port open a one-line editor
  with the current value; a first port that is not a number from 1 to 65535 is refused
  with the reason.
- A new interface or port moves the page to a new address, and the pane shows the new QR
  code.
- [Explore page: network and tunnel](explore-page-sharing.md) says what the network
  listener and the tunnel expose.
