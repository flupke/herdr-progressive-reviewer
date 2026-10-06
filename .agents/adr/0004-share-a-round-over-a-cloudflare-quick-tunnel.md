# Share a round over a Cloudflare quick tunnel

The reviewer wants a coworker who is not on the same network to follow and run an Explore
round. The pane gets a switch that shares the running round over a Cloudflare quick tunnel:
the reviewer runs `cloudflared tunnel --url` (no Cloudflare account), reads the public
`https://….trycloudflare.com` address that `cloudflared` prints, and the pane shows the share
link and its QR code. Only the pane has the switch; it starts off, and is not saved.

The link carries the round's network token, the one of the phone's QR code
([ADR 0003](0003-serve-the-explore-page-on-the-network.md)), which opens only that round. The
loopback token, which the pane keeps across rounds and restarts, never goes through the tunnel:
the tunnel forwards to a listener of its own on this machine's loopback interface, which serves
the page's routes behind the network tokens only, and answers only the tunnel's host name over
HTTPS. That listener learns the host name before it serves anything and closes with the tunnel,
so no listener answers the name once the tunnel stops; the other listeners' Host and Origin
checks are unchanged. The network listener may be bound to another interface, or be off, so the
tunnel does not reuse it; the two share the round's tokens while both run, so the link and the
QR code carry the same token.

The tunnel lasts one round: it stops when the reviewer turns it off, when another round starts,
on a Reset, and when the reviewer closes. `cloudflared` runs as a fork of the reviewer, through
`reviewer-control fork-exec` like run-ahead's forks, so it stays in the reviewer's process group
and gets its parent-death signal.

Turning the tunnel off closes its listener and its page, and the coworker's open page refuses
any further request, but it does not end the round's token, which the network listener keeps
serving: a tunnel turned on again in the same round has a new address and the same token.

Accepted risks: anyone with the link has the reviewer's full rights on the round, Implement
included, which has the agent change code in the reviewer's repository; there are no roles and
no read-only mode for now. The page's content (the agent's questions, cited code, answers)
passes through Cloudflare. The link works until the round ends, so a leaked link is bounded by
one round, and the reviewer can stop it at once.

Considered: forwarding the tunnel to the network listener (it may be bound to a VPN or LAN
interface, or turned off, and would have to answer the tunnel's name beside its own); a named
tunnel (needs a Cloudflare account and DNS); a read-only role for viewers (deferred).
