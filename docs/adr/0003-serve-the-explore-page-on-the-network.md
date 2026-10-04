# Serve the Explore page on the network

The reviewer wants to do an Explore round from a phone or a tablet on the same
network. The open review pane serves the Explore page on a second listener, bound
to the address of one network interface (the interface of the route to the
internet, or the one a setting names), over plain HTTP. The pane shows the
address of the running round's page, or of the start screen's page while no round
runs, and its QR code.

The protection is a random token for each round, carried by the QR code. The
page trades it for a cookie, refuses a request without it, refuses a request
whose host is not the address the pane gave out (against DNS rebinding),
and refuses a request whose origin is another site. A new round gets a new
token; the token of a closed or replaced round opens nothing. While no round
runs, the page has a token for its start screen, so that a phone can start a
round; the round started next keeps that token, so the page that started it
stays connected, and a Reset ends it with the round. The page that sends the Reset receives
the start screen's new token in the response, so that the reviewer who reset from a phone can
start the next round there; a Reset in the pane hands it to no page. The page on this
machine keeps its own loopback listener and its own token, for the Herdr
action. The MCP endpoint stays on loopback: the network listener serves the
page's routes only.

Accepted risk: the traffic is not encrypted. A person who can read the network
traffic, on the same Wi-Fi for example, can copy the token and use the page as
the reviewer until the round ends. The page can start a round, answer the
agent's questions and authorize implementation. Implement sends the list to be
implemented as the reviewer edited it, and the page cannot tell an edit from any
other text, so such a person could have the agent make any code change they
write, in the reviewer's repository, with nobody at the desk. The per-round token
bounds that window to one round and the time before it on the start screen. A person who
holds a round's token and resets the round from the page receives the next start screen's
token, and can keep going round after round while nobody resets in the pane; a Reset in the
pane ends that.

Accepted risk on the same host: browsers keep cookies by host name, not by port, so
the browser sends the page's token cookie to every server on the same address that
it visits, such as a development server on another port of `127.0.0.1`, and such a
server can set cookies under the page's names. The page names its cookies after
its port, which keeps two reviewers on one address from overwriting each other's
token but does not hide it. A process of the same user can read the page's
address record anyway; the exposure is to servers of other users or containers on
the same address.

Considered: HTTPS with a self-signed certificate (every phone warns and needs
an exception for each new certificate); a pairing step with a code typed on the
phone (more friction on every round). A setting turns network access off for a
reviewer who does not accept the risk, and a setting picks the interface, such
as a VPN's, to keep the page off an untrusted network.
