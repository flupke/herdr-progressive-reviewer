# Serve the Explore page on the network

The reviewer wants to do an Explore round from a phone or a tablet on the same
network. The reviewer serves the Explore page on a second listener, bound to the
address of one network interface (the interface of the route to the internet,
or the one a setting names), over plain HTTP. The pane shows the address of the
running round's page and its QR code.

The protection is a random token for each round, carried by the QR code. The
page trades it for a cookie, refuses a request without it, refuses a request
whose host is not the address the reviewer gave out (against DNS rebinding),
and refuses a request whose origin is another site. A new round gets a new
token; the token of a closed or replaced round opens nothing. While no round
runs, the page has a token for its start screen, so that a phone can start a
round; the round started next keeps that token, so the page that started it
stays connected, and a Reset ends it with the round. The page on this
machine keeps its own loopback listener and its own token, for the Herdr
action. The MCP endpoint stays on loopback: the network listener serves the
page's routes only.

Accepted risk: the traffic is not encrypted. A person who can read the network
traffic, on the same Wi-Fi for example, can copy the token and use the page as
the reviewer until the round ends. The page can start a round, answer the
agent's questions and authorize implementation, so such a person could make the
agent change code. The per-round token bounds that window to one round and the
time before it on the start screen.

Considered: HTTPS with a self-signed certificate (every phone warns and needs
an exception for each new certificate); a pairing step with a code typed on the
phone (more friction on every round). A setting turns network access off for a
reviewer who does not accept the risk, and a setting picks the interface, such
as a VPN's, to keep the page off an untrusted network.
