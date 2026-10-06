# Unit tests

What makes a unit test worth keeping, and the tautological tests code review removes.

A unit test earns its place when a plausible bug in the production code it calls turns
it red. For each test, name that bug: when none exists, the test is tautological. It
checks nothing the code owns, whatever its name says.

## Tests that can go red

- The expected value is written by hand from the behaviour the code owes, or comes from
  an independent source: an external tool's output, a second code path.
- The assertion reads what production code produced: a return value, a file in the
  store, a prompt, a rendered frame, an emitted action.
- A format this repository owns, such as saved settings, store files or state keys, is
  compared with a hand-written JSON or hex literal. A rename then breaks the test as it
  would break the data already saved.
- A prompt test asserts what the code puts into the prompt: identity, answers, paths,
  ordering. A rewording of its instructions must not break a test.
- An edge-case test feeds an input where a panic or a wrong answer is plausible: a
  boundary, Unicode, an empty input.

## Tautological tests

Code review deletes a tautological test, rewrites it to assert what the code computes,
or, for a harness, marks it ignored. The kinds:

- **Mirror**: the expected value is computed by the code under test, or by a copy of
  its logic, so a bug lands on both sides.
- **Echo**: the test reads back a field it set, a getter of a constructor argument, or
  a clone.
- **Constant pin**: `assert_eq!(LIMIT, 16_777_216)`, or the literals of a `Default` or a
  `match` asserted back. Build the boundary input from the constant instead.
- **Library behaviour**: std (`mem::take`, `BTreeSet` deduplication), serde derives
  without custom attributes, strum, a dependency's own keymap such as edtui's Vim. The
  dependency's own tests cover them.
- **Fake echo**: every value asserted comes from a fake the test configured, through
  code that only passes it on.
- **Vacuous assertion**: true for every value the type can hold (`queued_ms >= 0.0` for
  a duration), or met by an error reply as well as a success.
- **Harness counted as a test**: a `#[test]` that a parent test runs as a child process
  and that returns early unless an environment variable is set. Mark it
  `#[ignore = "<what runs it>"]` and have the parent pass `--ignored`, as
  `stalled_startup_process` in `crates/reviewer/src/runtime/responsiveness.tests.rs` does.

## Redundant tests

Code review also deletes a unit test of behaviour an e2e test already covers, when the
unit test adds no edge case and no faster or more precise failure.

## Unit or integration

A unit test lives in a crate's `src/`, outside any module named `integration`, and runs
in `make test`. It never waits on the wall clock: no sleep, no production timeout or poll
interval sat through, no loop that polls until a deadline, no window that proves nothing
happened. Give the code under test its durations or its clock as values the test sets
(zero, or advanced by hand), and wake the test with an event: a channel, a condvar, or an
event the code already publishes. Prove that nothing happened by ordering: send a marker
through the same path and wait for it; what did not arrive before it did not happen.

An integration test drives real processes: a private Herdr, a stand-in agent, jj or git,
a language server, a spawned binary. It lives in a crate's `tests/`, or under a module
named `integration` when it needs the crate's internals, and runs in `make integration`;
`make check-changed` runs both kinds for the crates a change reaches. It waits on events
too: what a stand-in reports on its event socket, Herdr's event stream, the events of the
reviewer under test. Its only clock is a guard against a hang, which fires on a failure
and never delays a test that passes.

## Auditing a crate

Read each test next to the production code it calls, since the bug that turns it red
lives there. A test that is sound except for one tautological assertion keeps the test
and loses the assertion.
