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

Every `IsolatedHerdrServer` names an event socket to the stand-in agent and its forks; a
stand-in refuses to run without one. Each stand-in's events arrive in the order it wrote them;
events of different stand-ins have no order between them. A `Disconnected` event says that a
stand-in's process ended, however it ended, a fork the reviewer stopped included.

`StandInEvents` keeps every event it received. A test waits on them in turn with
`events_until` and `wait_for`, each going on after the events the previous wait went through,
or on all of them together with `wait_until`: the prompts the agent read
(`IsolatedHerdrServer::prompts`, `wait_for_prompts`), the sessions it resumed, the forks that
started. To wait for what a new agent process reports, a test counts the events received
before it starts it, and looks only after them.

The stand-in agent of a second pane connects as `StandInRole::SecondAgent`
(`REVIEW_AGENT_E2E_ROLE=second`). The test's switches that must reach the agent before a
prompt, in the order Herdr types them, stay files in the server's directory, which the agent
reads as it reads the prompt: `swallow` makes it read each prompt without starting on it,
`unreported-resumes` keeps Herdr from hearing of its `/resume`.

### Proving that nothing happened

To prove that a prompt did not reach the agent, `IsolatedHerdrServer::mark` sends a marker
through Herdr, the path the reviewer's prompts take: what Herdr wrote in the pane before the
marker, the agent reported before it. A prompt the reviewer is still to send comes after the
marker, so the test first lets the reviewer send what it has: `ReviewFlowFixture::settle` and
`ConversationFixture::settled_prompts` flush the repository worker, whose Explore session
queues prompts, then the thread worker's prompt sender, whose courier types them, before the
marker.

### Turns

Herdr sees the stand-in agent's turns through its title. A turn ends as soon as Herdr reported
the agent working since the turn started, or, while the test holds turns (`hold_turns`, which
returns once the agent says `TurnsHeld`), when the test ends it. An agent whose state is
reported to Herdr ends its turns at once, since Herdr does not read its title, until the test
releases it (`release_agent`): the test tells the agent (`Release`), which says `Released`.
Herdr says that it released an agent (`AgentDetected` with `released: true`) only when it did
not detect the agent by name yet, which a loaded machine makes rare. Herdr 0.9.3 ignores a
reported `working` state of an agent it detects by name, so a reported agent cannot show its
turns: a prompt to it stalls for Herdr's 5 seconds.

Herdr releases an agent whose process ended (`AgentDetected` with `released: true`), and
detects by name an agent that starts where it released one before, whatever the agent reports:
a test restarts an agent with `AgentLifecycle::Native`. After a release, Herdr reports no agent
in the pane until it detected it again.

The run-ahead tests hold every turn of the agent in the pane until the test submitted it, as an
agent does that calls the reviewer's tools during its turn: forks are taken once Herdr reports
the agent idle after that turn, and an answer never finds the agent busy.

### Screen and title

The agent shows the screen the test gave it (`ShowScreen`) before the title (`ShowTitle`):
once Herdr reports the status a title shows, it shows the screen drawn before. Herdr sends no
event when a pane's screen changes, so this is how a test knows that Herdr shows a screen.

The test harness that runs the stand-in agent writes to the process's standard output, such as
its warning that a test ran for over a minute; the agent takes the pane from the standard output
at its start and leaves that output nowhere.

## Waiting on what the reviewer saved

`SavedState` watches the reviewer's state directory with inotify: a test checks what the store
says now and after each change, such as a turn's dispatch state or a fork's end. Run-ahead
records each fork's process before it takes another input, so once the forks reported
`ForkStarted`, flushing the repository worker (`Effects::flush`) leaves their records written.

## Waits the tests inject

- `ForkWaits` (claude-fork, through `RunAheadSetup.waits`): the run-ahead tests ask Herdr
  where the agent stands every 10 ms, and the switch that Herdr never confirms waits 2 s for
  Herdr instead of 20 s.
- `HerdrClient::with_prompt_start_timeout`: the kickoff the agent does not start on waits
  200 ms for Herdr instead of Herdr's 5 seconds.

## What only Herdr's own timing decides

Herdr's first `idle` for a newly detected agent, above, costs each test that starts or restarts
an agent about 4 s, and Herdr takes 0.3 to 0.6 s to type a long prompt; the tests wait for
these events, which no injected value shortens.
