# Test stand-ins

How tests run a private Herdr with a stand-in agent and stand-in forks, and wait on what these
do instead of polling. The rule they serve is in [Unit tests](unit-tests.md). The types are
documented in `review_test_support` (`HerdrTestServer`, `HerdrEventWatch`, `AgentStatusWatch`,
`stand_in`) and in `crates/reviewer/src/runtime/stand_in_agent.tests.rs`.

## The Herdr test server

`HerdrTestServer::start` returns once Herdr printed `herdr server running` on its stderr,
which Herdr does after its API socket is bound. The server runs under a shell that stops it
when the test lets go of the shell's stdin, on drop or when the test process dies, and then
removes the server's private directory.

Subscribe to Herdr's events before the action whose event you wait for: a subscription holds
once the call returns. Herdr detects an agent within about half a second, but reports a newly
detected agent's first `idle` status about 3.5 s later: wait for `AgentDetected` when the
detection is what counts.

## The stand-in event socket

A server started with `IsolatedHerdrServer::start_reporting` names an event socket to the
stand-in agent and its forks. Each stand-in's events arrive in the order it wrote them; events
of different stand-ins have no order between them. A `Disconnected` event says that a
stand-in's process ended, however it ended, a fork the reviewer stopped included.

To prove that a prompt did not reach the agent, `IsolatedHerdrServer::mark` sends a marker
through Herdr, the path the reviewer's prompts take: what Herdr wrote in the pane before the
marker, the agent reported before it.

### Turns

Herdr sees the stand-in agent's turns through its title. With an event socket, a turn ends
as soon as Herdr reported the agent working since the turn started, or, while the test holds
turns, when the test ends it. An agent whose state is reported to Herdr ends its turns at
once, since Herdr does not read its title, until Herdr reports it released.

Without an event socket, a turn lasts 1.5 s on a timer, and files drive the stand-ins
(`prompt.state`, `prompt.screen`, a fork's `.submitted` and `.end`). The Herdr tests still use
them; their migration to the socket removes the timer and the files.
