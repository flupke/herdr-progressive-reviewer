# Explore page: network and tunnel

How the page is served beyond this machine, what the network listener and the Cloudflare quick tunnel expose, and how tests stand in for them.

## The network listener

The host also serves the page on a network interface, for a phone, on the same
thread and runtime: a second listener with its own host name and a new token for
each round (`PageHost::share`). While no round runs, the listener has a token for
the start screen, which the round started next keeps, so the page that started
it stays connected. The pane saves one Explore page setting at a time
(`ReviewStore::save_explore_page_setting`, which keeps what another reviewer saved;
[Explore settings](explore-settings.md)), and `PageSharing` applies its network part at
once; once the page is taken off the network, no address of the old listener reaches the
pane. Herdr test servers write a `settings.json` that turns network access off into their
private state directory (`HerdrTestServer`); a `make vision` session writes the loopback
interface and any free port into its own (`write_reviewer_settings`), so the
pane shows a QR code that only this machine can open. Test sessions open no browser: they
set `BROWSER` to a stand-in.

## The tunnel

The pane's `O` shares the running round over a Cloudflare quick tunnel
([ADR 0004](../adr/0004-share-a-round-over-a-cloudflare-quick-tunnel.md)). The tunnel's
process lives in [`crates/quick-tunnel`](../../crates/quick-tunnel): `QuickTunnel` runs
`cloudflared tunnel --no-autoupdate --url http://127.0.0.1:<port>` as an `agent-fork` fork,
through `reviewer-control fork-exec`, reads the `….trycloudflare.com` host name from its log
on the standard error, and gives up after 30 seconds. The host
(`crates/review-explore-page-host/src/network/tunnel.rs`) serves the page on the loopback
listener the tunnel forwards to only once the host name is known, and admits that name with
an `https://` origin and a `wss:` socket. Its rounds are the network listener's tokens
limited to the round the tunnel shares, and a Reset from the tunnel's page hands it no token.
The network listener and the tunnel share the tokens, made when either starts and closed
when both stopped. A tunnel is stopped on a thread of its own, which the page host waits for
when it drops. Tests use a stand-in script for `cloudflared` that prints the address as the
real one does; requests reach the tunnel's listener directly, with the headers `cloudflared`
forwards (the public `Host`, an `https://` `Origin`).

## What the network listener exposes

- The page on the network is a second listener, on the address of one network interface,
  over plain HTTP. It answers only the address the pane shows. The MCP endpoint is never
  served on the network.
- A round started while another runs gets a new address.
- After a Reset, the round's address is refused and the pane shows the address of the next
  round's page; the page that sent the Reset moves to that address itself.
- Anyone who can read the network's traffic can copy the token and use the page as the
  reviewer until the round ends, and on the start screen before it; a Reset from such a
  copy hands it the next start screen's token too
  ([ADR 0003](../adr/0003-serve-the-explore-page-on-the-network.md)).
- A reviewer that cannot serve the page on the network (no network address, an unknown
  interface, all ten ports taken) keeps the page on this machine.
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
