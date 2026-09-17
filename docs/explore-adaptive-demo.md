# Adaptive Explore acceptance — 2026-09-18

This is a fresh, isolated real-agent run of the adaptive working-copy implementation.
It does not reuse the older snapshot demos. The complete three responses, literal
reviewer inputs and controlled fixture are in
[`explore-adaptive-demo-evidence.json`](explore-adaptive-demo-evidence.json).

This archived run predates the later focused-evidence and MCP-delivery feedback.
Its original responses are preserved; see [that update's checks](explore-evidence-mcp-update.md)
for validation of the current evidence and transport behavior.

## Setup and observed reasoning

A disposable Git repository changed `store.py` from a bare-list format to a
version-2 envelope. The unchanged reader catches a rejected format, fetches from
an origin, writes the current format and returns the rows. An ignored deployment
file describes a shared origin and leaves server limits unknown. The changed
fixture tests were inspected as source, not executed.

A private Herdr server, socket, state directory, repository, reviewer pane and
Codex home were used. The real Codex agent used the existing authorized sign-in
and its existing conversation across all three turns. No live user panes or
ordinary review threads were used. The normal sandbox blocked the private socket;
the same demo command succeeded with socket access.

| Turn | Actual result |
| --- | --- |
| Initial scan | The agent described the writer/reader transition, origin dependency, rollback refill and malformed-envelope concern. It asked whether the files could contain records the origin cannot reconstruct. Reversibility was **unknown**. Data preservation, origin load, version coexistence and validation were pending. |
| “These files are disposable cache. The authoritative data is elsewhere.” | After examining the read/refill path, the agent retired **Record authority and recovery** with a reason tied to that exact message and source evidence. It kept origin availability, mixed-version access and malformed-envelope handling active. Reversibility became **two-way**, conditional on a usable origin and successful cache writes. It asked about external controls on simultaneous refills. `interpretation` remained `null`. |
| “What happens if every instance rebuilds at once?” | The agent directly explained independent fetches, overlapping calls within an instance, propagated origin errors, subsequent refills and another possible refill burst on rollback. It refined the shared-origin inquiry and blast-radius assessment, distinguishing a plausible outage from an established outcome. It asked whether concurrency is bounded outside the code. `interpretation` again remained `null`. |

The source catalog was extended with `extra1` for `.deployment/origin.json`, an
ignored file absent from the initial catalog. The agent discovered it during the
initial scan, before the concurrency question. Its reference persisted through
later requests and opened in the native evidence viewer with a yellow relevance
outline. The concurrency question refined an existing branch; the agent did not
create a redundant branch merely to match the illustrative scenario.

The visible Explore transcript retained the original question and both exact
reviewer contributions. The map showed the retired inquiry, its original pending
wording and retirement reason, alongside the surviving inquiries and the remaining
human Files obligation. Door and Blast radius appeared separately beside later
questions. The ordinary review count stayed **0/2 reviewed**. The fixture retained
exactly the intended two-file change, **+9/-4**; no proposed source fix was applied.

## Validation and subsequent repairs

The initial real-agent build exercised working-copy reads, full conversation
requests, agenda retirement/refinement, both assessments and new-source admission.
The final interaction repairs followed this run: retain correction identity when
resuming or revisiting a draft; permit deferred agenda refinement; accept context
after an initial questionless stopping point; anchor the viewport across earlier
replies and manual scrolling; keep opening and question corrections separate;
return keyboard focus from evidence to the opening composer; keep the direct
answer visible on advancement; and deduplicate an exact finding repeated by the
agent. These paths have deterministic regressions. The recorded agent outputs
remain unedited, including its repeated finding.

The required `NEXTEST_TEST_THREADS=8 make check` covers formatting, complexity,
workspace checking, Clippy, doctests and the full test suite. The normal sandbox
hit the documented private-endpoint `EPERM`; the same command was retried with
socket access and passed **729 tests**, with **5 optional tests skipped**. The
separate real rust-analyzer Explore navigation test also passed.
Standards and Spec reviews passed after the repairs, and `make install` completed.

This demonstration is source analysis of a controlled fixture, not a load test or
proof of production recovery. Origin capacity, orchestration, caller retry behavior
and file-sharing topology remain unknown. No fabricated scale or outage probability
was used. Human file inspection and the recorded validation fix remain outstanding
in the fixture. Progress remains in memory; source must remain unchanged during a pass.
