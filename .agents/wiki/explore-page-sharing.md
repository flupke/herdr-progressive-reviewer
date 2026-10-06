# Explore page: network and tunnel

How the page is served beyond this machine: the network listener and its tokens, and the Cloudflare quick tunnel.

## The network listener

The host also serves the page on a network interface, for a phone, on the same
thread and runtime: a second listener with its own host name and a new token for
each round (`PageHost::share`). While no round runs, the listener has a token for
the start screen, which the round started next keeps, so the page that started
it stays connected. It announces the page's address to the pane each time the
token or its round changes, and the pane draws it with its QR code
([`crates/ui-qr-code`](../../crates/ui-qr-code)). When the listener cannot start, the pane
says why in place of the address (`ExplorePageNotShared`), with no toast. The settings
are the reviewer's Explore page settings (`ExplorePageSettings` in
[`crates/review-explore-page-settings`](../../crates/review-explore-page-settings), saved by
`ReviewStore` in `settings.json`; [Explore settings](explore-settings.md)). The pane
changes one at a time and sends it as `SettingsAction::SaveExplorePage(ExplorePageSetting)`;
the runtime saves that setting alone (`ReviewStore::save_explore_page_setting`, which keeps
what another reviewer saved), sends the pane the settings as saved
(`ExplorePageSettingsLoaded`), and `PageSharing` applies their network part at once: it moves the page to a new listener
(`PageNetwork::share`) or takes it off the network (`PageNetwork::unshare`, then
`ExplorePageOffNetwork`, after which no address of the old listener reaches the pane). Herdr
test servers write a `settings.json` that turns network access off into their private state
directory (`HerdrTestServer`); a `make vision` session sets the loopback interface and any free
port instead (`HerdrTestServer::set_explore_page_settings`), so the pane shows a QR code that
only this machine can open. Test sessions open no browser: they set `BROWSER` to a stand-in.

## The tunnel

The pane's `O` shares the running round over a Cloudflare quick tunnel
([ADR 0004](../adr/0004-share-a-round-over-a-cloudflare-quick-tunnel.md)):
`ExplorePageAction::OpenTunnel` and `CloseTunnel` reach `PageSharing`, which calls
`PageNetwork::open_tunnel` and `close_tunnel`; the pane hears `ExplorePageTunnel` with a
`TunnelState` ([`crates/review-explore-page-tunnel`](../../crates/review-explore-page-tunnel)):
opening, open with the link, failed with one line, or off. The tunnel's process lives in
[`crates/quick-tunnel`](../../crates/quick-tunnel): `QuickTunnel` runs `cloudflared tunnel
--no-autoupdate --url http://127.0.0.1:<port>` as an `agent-fork` fork, through
`reviewer-control fork-exec`, reads the `….trycloudflare.com` host name from its log on the
standard error (`ForkOutput::error_line`), and gives up after 30 seconds. The host
(`crates/review-explore-page-host/src/network/tunnel.rs`) binds the loopback listener the tunnel
forwards to, and serves the page there only once the host name is known, with `Hosts::tunnel`,
which admits that name with an `https://` origin and a `wss:` socket; its rounds
(`TunnelRounds`) are the network listener's `RoundTokens` limited to the round the tunnel
shares, and a Reset from the tunnel's page hands it no token. The network listener and the
tunnel share the tokens, made when either starts and closed when both stopped. A task stops the
tunnel when the published round changes; a tunnel is stopped on a thread of its own, which the
page host waits for when it drops. Tests use a stand-in script for `cloudflared` that prints the
address as the real one does; requests reach the tunnel's listener directly, with the headers
`cloudflared` forwards (the public `Host`, an `https://` `Origin`).

## What the network listener exposes

- The page on the network is a second listener, on the address of one network interface,
  over plain HTTP. It answers only the address the pane shows. The MCP endpoint is never
  served on the network.
- A round started while another runs gets a new address.
- After a Reset, the round's address is refused and the pane shows the address of the next
  round's page; the page that sent the Reset moves to that address itself. A page whose
  round was reset or replaced while the phone slept says that its address no longer opens
  a round.
- Anyone who can read the network's traffic can copy the token and use the page as the
  reviewer until the round ends, and on the start screen before it; a Reset from such a
  copy hands it the next start screen's token too
  ([ADR 0003](../adr/0003-serve-the-explore-page-on-the-network.md)).
- A reviewer that cannot serve the page on the network (no network address, an unknown
  interface, all ten ports taken) keeps the page on this machine, and the pane says why in
  one dim line where the address would be.
- A pane too narrow for the QR code shows the address alone.
- A firewall that drops incoming connections blocks the phone: the ten ports from the
  first port (8790 to 8799 by default) must be open to the local network.

## What the tunnel exposes

- The tunnel needs `cloudflared` on the `PATH`, and no Cloudflare account. Only the pane
  can turn it on or off; the page cannot.
- **Full rights.** Anyone with the link uses the round's page as the reviewer does: answer,
  write in the chat, cancel answers, reset the round, and Implement, which has the agent
  change code in the repository with the list they send. There are no roles and no
  read-only mode.
- **The round's network token.** The link carries the token of the phone's QR code, which
  opens only the running round. The token of the page on this machine, which the pane
  keeps across rounds and restarts, never goes through the tunnel.
- **Cloudflare sees the page.** The traffic is encrypted between the browser and
  Cloudflare, and between Cloudflare and `cloudflared`, but Cloudflare forwards the page's
  content: the agent's questions, the code as the page shows it, and the answers.
- The link can take a few seconds to answer after it shows, while Cloudflare registers the
  tunnel; until then a browser that opens it may show a Cloudflare error page.
- The tunnel lasts one round. It stops, and the link stops working, when the reviewer
  turns it off, when a new round starts, when the round is reset, and when the reviewer
  closes; the `cloudflared` process never outlives the reviewer.
- Turning the tunnel off closes the link, not the round's token: the phone's page keeps
  working on the network, and a tunnel turned on again in the same round gives a new
  address with the same token. A new round, or a Reset, ends the token.
- When `cloudflared` is not installed, fails to start, stops, or prints no address within
  30 seconds, the pane says so in one line where the link would be, with where to install
  it when it is missing.
