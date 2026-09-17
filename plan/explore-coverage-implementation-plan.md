# Explore — coverage-gated completion

**Next-slice implementation update · 2026-09-21 · updated with coverage-header UX**

Extend the existing durable Explore implementation. This is an update to the shipped tool, not a replacement plan for its UI, conversation, or persistence.

**Latest addition:** a pinned, clickable coverage percentage on every question and the conclusion page. It opens a file overview with explored, partly explored, and unexplored changes. Section 7 specifies the interaction and accounting; sections 9–12 include its state, implementation steps, tests, and demonstration. The underlying coverage and file-marking policy is unchanged.

## 1. Assignment and authoritative changes

Make an exploration account for the entire significant portion of its diff, then automatically mark its files reviewed using the existing file-baseline mechanism.

The agreed contract is:

```text
Agent submits a question
  essential evidence: minimal argument shown to the human
  supporting evidence: exhaustive related regions, collapsed by default
       |
       v
Reviewer accepts the question and returns remaining coverage work
  omit regions Jev classified as insignificant, when Jev is enabled
       |
       v
Human answers
  credit essential + supporting regions automatically
  save that coverage with the answer
  do NOT change any file-review baseline
       |
       v
Agent judges the exploration finished and submits a conclusion
  significant regions still unexplored -> error with remaining work
  none remain                         -> save conclusion + mark files at R
       |
       v
Later explicitly authorized fixes change code to R'
  ordinary Files baselines/interdiffs expose the new changes
```

These decisions supersede earlier proposals:

- Answered-question coverage replaces the earlier rule that evidence never counts toward review. **No separate agent declaration of reviewed ranges, human confirmation, or second acceptance click.**
- Both essential and explicitly supplied supporting evidence receive equal coverage credit. Supporting material does not have to be opened or individually scrolled past.
- Jev is a **significance prefilter for coverage gaps**, not a seven-facet startup-priority engine, general bug reviewer, or agent-directed classification framework.
- **A nonempty `TYPESAFE_API_KEY` in the reviewer process enables Jev.** Missing or empty means no provider calls and no effective Jev exclusions. Do not add per-repository consent switches, another settings screen, or a setup wizard. Document the source-sharing effect of providing the key.
- Unknown, failed, missing, and oversized classifications remain required.
- A pinned **Coverage** control shows progress on every question and the conclusion page; clicking it opens a file overview and direct navigation to remaining gaps. Its numbers come from the same ledger as the conclusion gate, not an agent estimate or a question counter.
- A small expandable debug area on every question exposes Jev exclusions and permits requiring a region during the active interview. The file overview links to this same debug view; it does not duplicate classification state.
- `submit_conclusion` must reject outstanding required coverage. The agent's judgment and this application-owned condition are both necessary to finish.
- File marks are written only as an effect of a valid conclusion, never incrementally as questions are answered.

The user knowingly accepts that a false `insignificant` classification can omit a material change. Make that dependence inspectable; do not replace this design with a different approval policy.

## 2. Baseline and integration facts

PR #3 was checked at head `44ceca7049043594619a762610f8dec818ad68a1` on 2026-09-21. It contains the approved durable slice 2 implementation. Recheck the local jj change and authorized subsequent edits before starting; do not reset the checkout to this remote commit. This planning inspection did not run tests. [R1]

Read `AGENTS.md`, `CONTEXT.md`, the current change description, the slice 2 plan including its scope override, `docs/explore-slice-2-handoff.md`, and `docs/explore-slice-2-recovery.md`. Then read the current usage, MCP documentation, and interview prompt.

Useful facts verified in this baseline:

| Existing owner | Reuse / change required |
| --- | --- |
| `review-explore/src/interview.rs` | `Question.evidence` and `Question.supporting` already exist. `ReviewerAnswer` retains the exact question/version. Reuse these rather than adding a third coverage list to every question. [R2] |
| `review-explore/src/capture.rs` | `Comparison::prepare` already obtains the comparison and native diff data. `context`, `diffs`, and `sources` are not serialized. Persist compact coverage geometry separately, not those source buffers. [R3] |
| `Exploration::unmapped` / `Comparison::maps` | These are topic-navigation associations, not full line coverage. A single overlap can map a hunk, and restored mapping can succeed without hunk context. They must not become the completion test. [R2, R3] |
| `review-mcp/src/lib.rs` | The current Explore response contains only `applied`. Extend the existing submission result to carry coverage feedback. [R4] |
| `review-state/src/review.rs` | Existing per-file baseline comparisons and caches must remain the way Files derives subsequent diffs. Its current `mark` helper obtains the current identity; automatic completion needs an explicit reviewed-version path instead. [R5] |
| `review-store/src/checkpoint.rs` | The store already accepts an explicit `baseline_commit_id`. Build on this, including its lossless path handling. [R6] |

Preserve one reviewer and one native agent conversation per repository, the existing human-readable answer wakeups, advertised Rust-derived MCP schemas, question history, separate conclusion/task editors, and explicit Implement authorization. Do not revive the abandoned MCP-refresh experiment.

## 3. Question evidence and answer semantics

### Two levels, not three

Keep the existing schema names:

- `next.evidence` is **essential evidence**: normally one to three focused snippets, each explaining what it establishes and why it matters to the answer.
- `next.supporting` is the **exhaustive additional set of regions related to the explanation/question**. Keep it collapsed and navigable on demand. Each region needs a short relationship, not another essay.

Together they account for all related changed code. They may also include unchanged callers, consumers, tests, or other explanatory context; only their intersection with the pass's actual diff contributes coverage.

Other citations in assessments, replies, agenda reasons, and topic entries do not silently grant coverage. Put a region into the question's essential/supporting lists when it is meant to be covered by answering that question. Do not use every citation ever collected as an implicit coverage claim.

Keep yellow outlines as relevance markers. Do not turn them into approval colors or show all supporting ranges as foreground windows.

### Credit the exact answered question

For a valid posted answer to question Q, record:

```text
credited(Q, answer) = changed portions intersecting
                     (Q.evidence UNION Q.supporting)
```

Use the exact saved question/version in that answer, not the currently focused question or a later version of its evidence. Commit answer and coverage attribution in the same durable update, before acknowledgement or waking the agent.

Coverage means **explored through an answered question**, not acceptance of the implementation. An answer requesting fixes counts. So do ordinary free-text answers and None of the above; do not introduce a semantic classifier to decide whether a response is sufficiently approving. Findings, interpretation, and follow-ups retain their existing separate meanings.

A specific working rule for the existing Defer action: explicit `deferred` input postpones rather than answers the question and earns no new coverage. Drafting, navigating, cancelling, and replies to a conclusion without a question also earn none. Explain this in help and tests. Do not otherwise condition credit on `TopicStatus::Accepted` or on receipt of the next agent response.

Idempotent retries, repeated answers, and overlapping references do not double-count. Corrections append history. Within an unchanged pass the required criterion is **explored at least once**: changing a topic's status, retiring a branch, or flagging reconsideration does not erase historical coverage or manufacture new coverage. The agent must still investigate unresolved semantic concerns before choosing to conclude.

A region merely assigned to an unanswered, retired, or superseded question is not explored. The active-agenda accounting must not hide an abandoned question's uncredited regions forever.

## 4. Application-owned coverage ledger

### Persist geometry, not source snapshots

Build the ledger from the selected comparison's real diff once at pass creation, using existing prepared data. Retain:

- Pass identity, Review unit, exact reviewed checkpoint, and the repository identity needed for later ordinary file marks.
- The exact changed-file set and old/new paths.
- Added-line intervals on the new side and deleted-line intervals on the old side.
- Explicit non-line change items: actual rename, executable/type changes, binary replacements, empty-file creation/deletion, submodule updates, or other supported metadata changes.
- Inventory completeness or explicit inability to enumerate a change.
- Answer-attributed interval coverage, Jev decisions, human overrides, and conclusion-finalization records.

Do not persist source bodies, whole rendered prompts, or renewable MCP access. Keep working-copy reads and historical base readers as they are. No new repository archive, filesystem freshness scan, source watcher, per-range checkpoint, or source remapping engine.

Parse changed rows, not just `@@` headers: context lines inside a hunk are not changed lines. Preserve additions and deletions independently. A replacement needs coverage of its relevant old and new sides; citing the new side is not automatic credit for everything removed.

The existing manifest includes a generic file entry for each path. Do not count that as an extra metadata obligation when it represents no distinct change. Conversely, a real permission or rename change must not disappear because text hunks were covered.

For textual evidence, null line ranges may remain valid for navigation but must not mean “all changed text.” Require explicit ranges to credit text. For non-line changes, allow the existing path/side plus null-lines evidence to identify matching change items, with a meaningful relationship. A rename-plus-edit can therefore need both file-level evidence for the rename and line evidence for its edits.

For an unavailable source, retain the coverage obligation or an explicit unresolved inventory limitation. Never turn failed loading into an empty diff. Non-text changes can be discussed at file level; their limitations remain visible in the conclusion.

### Set accounting

Use compact interval sets keyed by lossless path and side, plus sets of non-line change IDs. Names below describe the logic, not required public API names:

```text
D = all changed portions/items in this pass
C = union credited to posted answers
E = persisted Jev-insignificant portions, when Jev is enabled,
    minus portions overridden with Require review

required_unexplored = D - C - E

P = portions assigned to still-active unanswered questions
unassigned_required = required_unexplored - P
awaiting_answer     = required_unexplored INTERSECT P
```

The completion check uses `required_unexplored`, not `unassigned_required`.

Normalize unions and compute intersections/differences in application code. A one-line overlap never covers the rest of a hunk. Unchanged context contributes zero. Overlapping support for several questions is normal.

Expose disjoint totals to the coverage header/file overview and debug view: explored; currently excluded and not explored; required and not explored. Section 7 defines the displayed percentage and file groups using these same sets. Do not present Jev-excluded lines as lines the human discussed. This is coverage accounting, not a safety score.

Update the ledger incrementally. Do not reread the repository or reconstruct every conversation turn for every UI frame or submission. Persist enough geometry that reopening works with `Comparison.context` empty. Test a large newly added file and large overlapping lists without allocating a record per displayed line or repeatedly serializing source text.

## 5. Jev significance prefilter

### Enable switch and scope

Check `TYPESAFE_API_KEY` in the **reviewer process environment**. Missing, empty, or whitespace-only disables the provider. Do not log the value, store it in a pass, include it in wakeups, or use agent-shell availability as proof that the reviewer has it.

With Jev disabled, `E` is empty: every changed portion needs question coverage. Coverage tracking, MCP feedback, conclusion validation, and automatic file marking still work fully.

With a key, send only bounded source material necessary for the significance check, repository-relative references, and the rubric. Do not send the human interview, implementation-agent history, access tokens, unrelated files, credentials, or binaries. Document that setting the key enables this external source processing; do not add a second opt-in requirement. Preserve existing source-access protections.

### One narrow, versioned rubric

The application supplies a fixed, inspectable significance rubric for this slice. This is intentionally narrower than the abandoned generic prioritization plan: Jev decides whether a changed region needs explanation, not whether a whole system is correct or an API has no affected consumers.

Use Choice with three outcomes:

| Outcome | Meaning and effect |
| --- | --- |
| `significant` | The change contains behavior, policy, state, contract, dependency, or operational substance that should be explained. It remains required. |
| `insignificant` | The supplied context supports treating this particular change as mechanical/non-consequential material not requiring its own explanation. It can be excluded from outstanding coverage. |
| `uncertain` | Context or understanding is insufficient to decide. It remains required. |

Version the instructions and option criteria together. The user supplies the product policy; the reviewer encodes it; Jev applies it to bounded candidate regions. Do not ask the interview agent to prepare a separate packet for every candidate as a prerequisite for getting gap feedback.

Use functions and ordinary import/wiring edits as starting examples, not a universal syntax whitelist. A function change generally needs explanation; a plainly mechanical import adjustment may not. Imported targets, module side effects, dependency changes, compile flags, configuration defaults, or deleted assertions can matter. The rubric must distinguish those from incidental formatting/wiring. If deciding requires an unseen consumer or other investigation, return uncertainty rather than claiming that missing context proves insignificance.

Do not replace this filter with seven independent risk labels, numerical severity, blanket import exclusion, or an LLM-generated list of line numbers. Coordinates and candidates come from code.

### Inputs must fit without losing their meaning

Classify meaningful, localized change regions, not isolated words and not an entire PR. Start from contiguous change blocks with before/after code and bounded surrounding context. Reuse readily available syntax/enclosing-definition information where useful; do not build a new language index or call graph for this slice.

A classification target may contain both old and new changed intervals. Give it an exact code-computed ID and point to its state fields in the instructions. Store precisely which intervals its result applies to. One mechanical prefix does not justify excluding the entire function/file. Mixed significant and mechanical regions may conservatively remain significant unless they can be split without losing required context.

The official model page currently lists `jev-1.13.0`, with **32k tokens for state plus the longest question** and **64k for the complete request**. Those are different limits. Recheck the provider docs when implementing. [T1]

Budget context, instructions, and criteria together. Use a compatible token counter if available; a character/byte estimate must not be represented as an exact token count. Keep packets comfortably bounded, and handle provider size rejection as unclassified required work. Never silently truncate either side, drop a necessary helper, or classify the first portion and exclude the remainder. Oversized meaningful units stay required. Requests are independent; repeated required context must accompany each. The provider also documents weaknesses with indirection and irrelevant context, so treat missing context conservatively rather than trying to solve cross-file correctness with the filter. [T2, T4]

Use a small Rust HTTP adapter unless the checkout already has a suitable one. The documented endpoint is `POST https://api.typesafe.ai/v1/systemone` with bearer authentication, `state`, `model`, and typed `questions`. Validate response IDs, types, options, numeric fields, and target membership before applying exclusions. [T3]

Record the actual model, rubric version, input source references/omissions, selected result, probabilities/confidence when returned, and errors. Do not invent prose explaining a result Jev did not explain. Provider confidence is diagnostic, not proof of safety. [T2, T3]

### Scheduling and failure behavior

Run optional classification once per new pass's candidates, in bounded background work using the comparison already prepared. It may overlap the agent's initial investigation. Do not put HTTP work inside rendering, a store lock, or the synchronous MCP acknowledgement path.

At each submission, use the classification results already durably committed. Anything pending remains required and is labelled pending, not insignificant. Give extraction, individual requests, response size, concurrency, and the total job finite limits; document the chosen defaults. At the deadline retain completed results and leave the remainder required. Do not hold an interview hostage to the provider.

Do not reclassify everything on each question, answer, duplicate MCP call, view change, or reopen. Reuse results within the pass. No cross-pass cached exclusions or transfer to modified source in this slice. Cancel work for an obsolete pass; late callbacks must check pass and attempt identities before touching state.

Authentication failure, rate limit, timeout, malformed response, missing answer, context overflow, or uncertainty never excludes code. Suppress repeated identical provider-error spam and allow the ordinary interview to continue. No automatic unbounded retries.

On restart, retain existing completed classifications, but do not replay paid work automatically; interrupted/missing items remain required. If the current process lacks the key, stored classifications remain inspectable history but are not effective exclusions for a still-open interview. Do not retroactively undo a previously completed conclusion merely because a later process has different credentials.

## 6. Return coverage work through the existing MCP tools

Keep `submit_question` and `submit_conclusion`; do not add source-fetch tools, a mailbox, or another agent conversation.

### Successful `submit_question`

After normal validation and durable acceptance, return the accepted status plus a coverage feedback object. It must describe the ledger **after registering the new question but before that question has been answered**.

A simplified result shape:

```text
applied: true
coverage_revision: 12
required_unexplored: 3 regions
  unassigned_required:
    config.rs new 40-43
    worker.rs old 88-94
  awaiting_answer:
    current question q-retry/v2 -> retry.rs new 52-76
jev:
  mode: enabled
  excluded_unexplored: 5 regions
  pending_or_unclassified: 2 regions
```

Return exact path/side/range locators and non-line change items, not just counts or opaque IDs. The agent reads source/history through its existing tools. Unassigned work guides subsequent questions. Active unanswered assignments are shown separately so the model does not ask duplicate questions or mistake assignment for completion.

A bounded result is necessary for huge diffs. Coalesce contiguous intervals and use deterministic ordering. If the complete gap list exceeds the response budget, return an actionable bounded prefix with the **full remaining total and an explicit `has_more`**. As questions cover that prefix, subsequent submissions expose remaining work. Never claim the prefix is the whole inventory, and never use its length/emptiness as the completion calculation. Do not add an independent repository-catalog API merely to avoid a large response.

Tie accepted receipts to a coverage/classification revision. Identical MCP retries do not duplicate questions, credit coverage, resubmit Jev requests, or advance navigation again. Preserve the existing applied/replayed semantics. Changes from subsequent answers/overrides appear in later turn feedback; a receipt is not an eternal claim about the current ledger.

A Require review override can occur after a question's MCP response. Carry a compact coverage-revision/gap notice on the next ordinary answer wakeup, without losing its readable selected option and exact comment. This keeps the agent informed without an unsolicited wakeup or new transport.

### Blocked `submit_conclusion`

Validate ordinary identities, final interpretation, and the proposed update first, against a candidate state. Then evaluate the complete required-unexplored set from the current ledger and overrides.

If nonempty, return an MCP tool error with a stable error kind and actionable locators:

```text
coverage_incomplete
Significant or unclassified changes still need an answered question:
  config.rs new 40-43
  worker.rs old 88-94

Continue with submit_question using the same pending request/answer.
No conclusion or file-review marks were saved.
```

A coverage error must not consume the outstanding request, partially append the conclusion, erase the final answer, or apply its interpretation twice. The human answer is already durable; the agent can respond to that same turn with another question and preserve all original decisions.

An incomplete inventory is also a blocker. Distinguish it from a genuine empty diff. Unknown/unclassified regions are required without waiting indefinitely for Jev.

There is no `force`, agent-supplied `all_covered`, or partial-conclusion flag that bypasses marking conditions. Stopping early uses the existing cancellation/close and retained history; it does not mark files or fabricate a successful conclusion.

Zero outstanding ranges do not auto-generate a conclusion. The agent may still examine interactions, unresolved choices, and risks. Its explicit conclusion submission supplies the second, semantic judgment. Conversely, an all-insignificant/no-change pass need not manufacture a meaningless question merely to satisfy a question counter.

## 7. Coverage header, file overview, and Jev debug zone

### Pinned coverage control

Add a compact **Coverage** control at the top of Explore, on every question and the separate conclusion page. Keep it visible while the question, evidence, and answer scroll below it. Reuse the existing pinned navigation area rather than adding a large progress dashboard or another permanent help/warning strip.

Proposed shape, with illustrative values:

```text
Explore     [Coverage 42% v]                  [Previous] [Next]
```

For a newly started pass with required changes, the first question shows **0%**. Posting an answer updates the indicator from the durably credited essential/supporting ranges, without waiting for the agent's next question. Merely presenting a question, reading its code, changing the selected page, or drafting an answer earns no credit. Retain the existing no-credit rule for explicit Defer.

A successful conclusion shows **100%**. Reaching 100% before the agent concludes is allowed: it means the coverage condition is satisfied, not that all consequential discussion is finished. Do not automatically conclude, navigate, approve choices, or mark files when the number reaches 100%.

Before a usable inventory exists, show a compact preparing state rather than invented progress. In an active pass, the pinned value describes the **current pass**, including when browsing its earlier questions; changing the history selection must not make earned coverage appear to disappear. Historical per-turn accounting remains available through the saved revision references. Previous/completed passes use their own accounting and, after successful completion, their finalization receipt—not a new calculation that applies another pass's state or the current process's changed Jev enablement.

### Percentage and edge cases

Derive the percentage in application code from the same full ledger and effective exclusions used by `submit_conclusion`. Do not average file percentages or use the number of questions, evidence snippets, hunks, or returned MCP gap entries.

For display, one changed added/deleted line is one unit, and each distinct non-line change item is one unit. Count interval lengths, not individual line objects. This equal weighting is a progress-display convention, not a risk weighting; the gate still requires the exact remaining set to be empty.

Using the sets from section 4:

```text
required         = D - E
explored_required = C INTERSECT required
remaining        = required - C   // exactly required_unexplored

if inventory is complete and required has nonzero weight:
  percent = floor(100 * weight(explored_required) / weight(required))
```

Keep the underlying integer counts exact. **Never round an incomplete pass or file up to 100%.** A removed line or an unaddressed metadata item can keep the indicator below 100% even when every displayed new-side snippet has been discussed.

Jev exclusions reduce the required denominator; they are never added to the explored numerator. The expanded overview must show the separate excluded count and allow inspection. Raw answer coverage remains recorded even when some of that code is also classified insignificant.

With the required set fixed, answers only increase or preserve coverage. Classification results can change that denominator; **Require review** or reopening an active pass without the key can add required work and lower the percentage. Show the actual recomputed value, preserving the answer history and draft. Do not fake monotonic progress or manufacture answered coverage to conceal that change. The expanded counts/debug view must make it clear why required work changed.

Handle exceptional cases explicitly:

- **No required units:** before conclusion, show `Coverage — · No required changes` with either `No diff changes` or `All changes excluded by Jev` as appropriate. Do not divide by zero, fabricate an answered question, or call excluded-only material explored. A valid accepted conclusion may show `Coverage 100% · No required changes`, retaining the explanation.
- **Incomplete or unavailable inventory:** show `Coverage incomplete` / `Coverage unavailable`, with known gaps and the limitation accessible. Never display an unqualified 100% from a partial denominator; the existing conclusion blocker remains in force.
- **Coverage complete but file marking pending/failed:** the indicator can say 100%, but the existing completion status must still show saving or the marking error. Do not claim the file marks succeeded or enable Implement prematurely.

### Clickable file overview

Click the coverage control, or activate it with the keyboard, to open a compact expandable overview. Keep it collapsed by default. A proposed example (100 required units in total) is:

```text
Coverage 42%                    42 / 100 required change units

Explored
  retry.rs      100%   10 / 10

Partly explored
  worker.rs      50%   32 / 64   [Next unexplored region]

Unexplored
  config.rs       0%    0 / 26   [Next unexplored region]

Jev exclusions · 6 regions      [Inspect]
```

Show the pass's **entire changed-file set**, independent of Files filters and manual review badges. Unchanged supporting sources are not diff files and must not inflate this overview.

Compute each file's percentage from its own required changed units using the same formula. Group files as follows:

- **Explored:** all required changes are credited to answered questions. Where there are also excluded changes, show their separate count; 100% does not mean the excluded lines were discussed.
- **Partly explored:** some, but not all, required changes are credited.
- **Unexplored:** required changes remain and none have received answer credit.
- **Excluded by Jev:** a file with no required units because all of its changes are excluded. Keep it visible and inspectable, but do not label it explored or assign a misleading per-file 100%. Files with inventory limitations show that state instead of a computed completion claim.

Rows expose their exact added/deleted and non-line counts in compact details when useful. Use **explored**, not **marked reviewed**, before conclusion: these are discussion states, not early writes to Files baselines. A manual Files mark must not silently credit the Explore ledger either.

Selecting a file opens its full available pass diff in the existing native viewer, not a newly rendered static snippet or a separate dashboard. **Next unexplored region** jumps to the next required uncredited interval/item, including old-side deletions and non-line changes; cycle deterministically within the chosen file. Jev exclusions have their own inspection path. Native search, selection, comments, resizing, and new-side LSP remain available under the existing source limitations.

Opening, closing, or navigating this overview must preserve the current question, selected answer, exact unposted text, and the previous question-evidence position. Provide an immediate return to the question's primary evidence through the existing navigation. A coverage-navigation target is not a change to the question's evidence lists and earns no credit. Missing historical/source content shows a local limitation without changing the ledger.

Support mouse and keyboard without intercepting answer-editor or code-navigation input. Keep the overview scrollable on large diffs and usable on narrow terminals; do not expand all files automatically. Recompute groups/counts on committed ledger events, not by reparsing the diff every frame. If a focused file moves to a new group after an answer or override, retain focus by stable file identity, not its old row number. No automatic agent wakeup, Jev call, or review marking on inspection.

### Jev exclusions on every question

Keep the agreed small collapsed block in the existing vertical question flow. The header overview's **Jev exclusions → Inspect** opens this same exclusion data/view. Do not replace the UI or expand exclusions into the primary evidence list.

```text
> Jev exclusions · 6 currently uncovered regions omitted

Expanded:
  worker.rs   old 12-14 / new 12-15
  [Inspect diff]  [Require review]

  config.rs   new 8-10
  [Inspect diff]  [Require review]
```

The default active view shows **currently unexplored regions excluded by Jev**, not just new exclusions since the last turn. Excluded regions later covered by answers leave this outstanding list, while their original classification remains in history.

Inspect uses the native fully functional source/diff viewer, with proper side/range coordinates and existing resizing, search, comments, and new-side LSP. Further details show the actual criterion, classification result, model/rubric, input references and omissions, and returned diagnostic values. Do not invent a rationale. Source remains the existing working-copy/history view, not a newly archived copy of the provider input.

Require review:

1. Saves a reviewer override immediately for that region for the rest of the active interview.
2. Removes it from effective exclusions, including against late provider responses.
3. Returns it to required work only to the extent not already covered by an answered question.
4. Makes it block conclusion until covered; does not erase existing answer coverage or require a confirmation dialog.

Keep the question, answer draft, viewer position, and focus stable when counts/results change. A committed Require review override also updates the pinned percentage and file groups from the same ledger revision. Expanding the zone or inspecting an exclusion never calls Jev or marks files.

Retain the classification/coverage revision visible when each question was accepted. History can expose **exclusions at that turn** separately from the active pass's current exclusions. Do not copy the whole exclusion set into every question; store immutable receipts and revision references. Completed/previous-pass history is inspectable, not a backdoor for rewriting its completed review certificate. Active overrides occur before successful conclusion; use existing Files unreview/New pass controls for additional work after completion.

When disabled, a compact debug state can say `Jev disabled — all changes require coverage`. Failed and pending classifications appear as required-work diagnostics, not as exclusions. No persistent warning strip or classifier dashboard.

## 8. Successful conclusion and exact file baselines

### Preserve checkpointed-diff efficiency

Only the Explore ledger is partial. Ordinary file baselines remain unchanged until completion:

```text
Questions 1..N at R:    update Explore interval coverage only
Accepted conclusion:  ordinary complete-file marks at R
Later code at R':      existing Files comparison against R
```

Do not introduce partially materialized files, per-hunk jj revisions, synthetic mixed trees, or a custom residual-diff renderer. Reuse baseline grouping, cached comparisons, and jj interdiff behavior. Mark only the changed-file set of the reviewed comparison, including filtered/already-reviewed files in that pass; unchanged supporting files are not marking targets.

Use the **exact reviewed baseline identity**, not whichever revision happens to be current when a callback or retry executes. Add a narrowly named explicit-checkpoint marking operation or call the existing store through the appropriate owner. Do not blindly loop over the current `ReviewTracker::mark`, which reads a fresh current identity. [R5, R6]

Preserve the different Git and jj meanings of Review unit and Checkpoint. Reuse the repository's existing baseline-compatible identity; do not manufacture a Git revision from a transport-only identifier. Failure to obtain/reopen the required baseline is an error, not permission to mark a later version. The working-copy-unchanged assumption remains; this is identity-correct storage, not a new freshness-scanning feature.

### Durable finalization, not half a successful tool call

A valid conclusion now has a multi-file local side effect. Extend existing durable transactions with a small completion record so crashes cannot cause false success or mark a newer version:

```text
Validate candidate conclusion + current coverage
  -> save pending completion with exact payload, targets, baseline and identity
  -> apply ordinary file marks idempotently
  -> commit completed conclusion/marking receipt
  -> refresh Files through existing events and acknowledge MCP/UI
```

Validate all coverage and target identities before writing any mark. A coverage rejection writes no marks.

The completion record must support recovery after any subset of path records was written, without duplicating conclusions or advancing a baseline. Because per-file records are separate atomic writes, do not describe the batch as physically atomic unless the implementation actually makes it so.

During finalization, prevent a late override or new answer from slipping between coverage validation and its committed completion state. Serialize local operations through the existing owner/locking approach and reject stale actions clearly.

Use expected prior marks and exact targets, or equivalent existing serialization, to prevent an old completion retry from overwriting newer manual or later-pass marks. A matching target is already applied. A conflicting later record is not permission to overwrite it; report the conflict and retain recovery state.

Do not return successful conclusion acknowledgement or enable Implement as if completion succeeded while file marking failed. Preserve the candidate conclusion, answer, and marking progress; expose a local retry/recovery error. Retrying the identical submission or reopening can finish already-authorized **local marking**, but never sends another agent prompt or implementation request automatically.

An identical successfully completed conclusion retry is acknowledged without reapplying file marks—even if the user later manually unreviews a file. Further conclusions in an already finalized unchanged pass do not repeatedly reset those marks. Preserve distinct conclusion history and current-task editing.

Successful conclusion does not authorize source edits. The existing explicit Implement action is still the only authorization to apply the human-edited task list. Marking precedes that action; resulting edits are therefore visible against the reviewed version.

### Do not silently change the next pass's scope

The checked baseline starts Explore over the full base-to-working-copy change, including reviewed files. This update leaves that selection policy unchanged. Files' residual diffs after marking work through their existing baseline machinery; a future residual-only Explore scope is not silently implemented here. The ledger always accounts for exactly the comparison selected for its own pass.

## 9. Persistence, protocol, and presentation updates

Keep new domain concepts small and owned by the existing layers. Suggested concepts, not mandatory spelling:

| Concept | Owns |
| --- | --- |
| `CoverageInventory` | Exact changed intervals/items and inventory completeness. |
| `AnswerCoverage` | Credited ranges attributed to immutable question/version and answer IDs. |
| `SignificanceResult` | Target, typed result, model/rubric, context references, diagnostics and attempt identity. |
| `CoverageFeedback` | Required gaps, current unanswered assignments, disjoint counts and revision. |
| `CoverageSummary` | Derived pass/per-file required and explored counts, percentages, and limitation states for the pinned header/overview; not an independent coverage authority. |
| `CoverageOverride` | Human Require review entries. |
| `ReviewCompletion` | Exact accepted scope, baseline, prior-mark expectations and recoverable local effects. |

Use existing `CodeLocation`, path, checkpoint, question, and answer types. Define methods and invariants before handlers. Extract a small provider/domain crate only where ownership warrants it, not a generic classifier plugin framework.

Add overview expansion, selected file/region, scroll, and any return-to-question viewer position to the existing pass view state where needed. Keep this separate from coverage transactions and answer/editor ownership. Derive the header and row summaries from the durable ledger and its effective classification revision after restore; do not store a mutable percentage as another source of truth. Preserve the recorded finalization policy for completed-pass display. Use the existing saved-state/event pipeline, with no polling or automatic classification on view restoration.

Extend shared Rust types and their advertised schemas. Keep existing input names unless a small documented extension is necessary; MCP output now carries coverage. No hand-maintained giant schema examples in kickoff instructions. Normal restart/resume to load changed tool definitions remains the supported workflow; no app-server refresh experiment.

Update the interview prompt to explain the new contract:

- Explain the whole change through coherent questions, including exhaustive related supporting regions without flooding foreground evidence.
- Read uncovered-work feedback and group related gaps into useful questions; do not list unrelated files merely to satisfy a counter.
- Address old/deleted code as well as new code. Keep before/after identities accurate.
- Answering credits the explicit evidence lists automatically; topic entries and unsent research do not.
- Jev exclusions are coverage exemptions, not claims of correctness; the agent may still investigate them.
- Call `submit_conclusion` only when the agent judges the full material surface understood. On coverage error continue the same interview, preserving the latest answer and decisions.
- Remove unconditional wording that Explore never marks Files or that every successful conclusion still requires a second complete manual file pass. Explain automatic marking accurately, while keeping open concerns and implementation tasks honest.

Retain one-question-at-a-time presentation, concise options/free text, branch evolution, Door and Blast radius, essential-evidence curation, and the separate conclusion page. Do not change file visibility while an answer is being discussed; no marks have been written at that point. Keep evidence readable after conclusion rather than collapsing the historical question because Files now hides its diff.

Extend durable storage without adding legacy migrations. An old pass lacking a trustworthy coverage inventory is **not** an empty, fully covered pass. Preserve its bytes/history according to the existing unsupported-state policy and require a New pass for the new auto-marking workflow. Do not automatically reconstruct old coverage by scanning current files or crediting old topic associations.

## 10. Implementation order

### A. Ledger and contract, with Jev disabled

Implement exact changed-range/item inventory, answer-attributed unions, gap calculations, durable restoration, and response types. Reuse essential/supporting lists. Add the pinned coverage header, derived per-file overview, and native navigation to remaining gaps in this no-key stage. Keep ordinary file marks untouched.

Demonstrate 0% on the first question, then an intermediate value after answering one small essential snippet and a large collapsed supporting set. Open the overview, inspect a partly explored file and its next old-side gap, and return with the answer draft intact. Show a real uncovered deletion blocking the candidate conclusion.

### B. Conclusion gate and ordinary file marks

Implement the rejected-conclusion repair path, explicit-baseline batch finalization, idempotent local recovery, and Files refresh. Show that reaching 100% does not finish the interview or write marks by itself, then show 100% on the accepted conclusion with marking complete. Demonstrate post-completion edits becoming incremental diffs through the existing Git/jj paths. This provides the complete useful feature without any provider credential.

### C. Optional prefilter and debug override

Implement the versioned significance rubric, bounded request construction, background lifecycle, credential switch, persistent results, exclusion filtering, and per-question debug zone. Integrate effective exclusions and Require review overrides with the same header/overview calculations, including excluded-only files and zero-required cases. Start with deterministic provider fixtures; perform a small live request from the normal reviewer launch environment before claiming the real integration works.

Do not require another way of launching Codex/Claude, a sidecar, or a new MCP server to reach Jev. Source processing is owned by the reviewer.

### D. Regression, documentation, and demonstration

Exercise the combined flow on a representative multi-file change. Follow `AGENTS.md` for the fresh jj change, full checks, required review/repair loop, commit description, and installation. Use private Herdr/MCP/agent paths throughout; do not operate on live user panes as test fixtures.

Keep subsequent feedback fixes in this feature's change under the repository workflow. Stop after this coverage feature and its focused feedback; do not fold in task-completion tracking or the abandoned generic Jev pilot.

## 11. Required regression tests

### Coverage and question attribution

- Essential plus supporting ranges count after a durable ordinary answer, even with `needs_follow_up`; presenting a question, opening evidence, or drafting does not count.
- None of the above/free text follows the answer rule; explicit Defer adds no credit. Conclusion-only replies add no question coverage.
- Credit binds to the exact question/version answered; an answer to history does not cover a newer question's expanded ranges.
- Overlaps, duplicated citations, answers, corrections, and retries produce set unions without double-counting.
- Both sides of replacements and deletion-only regions work. Context lines, unchanged supporting files, and other assessment/topic citations do not inflate coverage.
- A tiny overlap does not credit a large hunk. One new-file hunk can be covered across several questions.
- Rename, permissions, empty-file, and non-text entries are represented without duplicate artificial file obligations. Textual null-lines evidence cannot blanket-credit a file.
- Incomplete enumeration or missing restored geometry cannot pass as zero remaining work.

### Submission and conclusion

- Each successful question returns post-registration unassigned work and unanswered assignments separately.
- Assigned-but-unanswered work blocks conclusion; an abandoned assignment returns to unassigned gaps.
- Coverage rejection leaves the same answer/request available; repair via another question works without duplicate interpretation or history.
- Output limits expose `has_more` and real totals. The final gate tests the full ledger, not the returned prefix.
- Existing successful retries remain idempotent, including lost UI acknowledgements and changed later ledger revisions.
- Full coverage does not itself finish the interview. A valid conclusion automatically marks exactly the pass's changed files, without human confirmation.

### Coverage header and file overview

- A nonempty required diff shows 0% at the first question, even if that question's essential/supporting lists mention the entire change. A durable answer, not displaying the question or receiving the next response, increases progress. Defer/drafts/navigation do not.
- When evaluated at the same ledger revision, header, per-file rows, MCP gaps, and conclusion validation agree. Historical MCP receipts keep their recorded revision rather than being silently rewritten. Unequal-sized files demonstrate that the total is not an average of file percentages. Count both diff sides and distinct non-line items; overlaps and unchanged context do not inflate it.
- Flooring never displays 100% while any known required unit remains. Include one deleted line and one metadata item left after a large textual change is otherwise covered.
- Jev exclusions reduce the required set but are never called explored or added to the numerator. Partially excluded files disclose the distinction; excluded-only files remain visible outside the Explored group. No-key, all-excluded, empty-diff, and incomplete-inventory states follow section 7.
- Require review and changed active-pass key availability can lower displayed progress without erasing answer coverage. Completed historical totals use their finalization receipt and are not rewritten by a later environment.
- The header stays pinned while conversation/evidence scroll. Expansion and file navigation work with mouse/keyboard and narrow layouts. File rows stay focus-stable if grouping changes.
- Selecting a file opens the full native diff; Next unexplored region reaches added ranges, deletions, and metadata, including locations outside primary evidence. Returning preserves the question, selected option, unposted text, viewer position, and manual height. Inspection never grants coverage or sends work.
- Restoring header/overview state does not regenerate the inventory, invoke Jev, resend prompts, or overwrite independent question/conclusion editors. Current-pass history navigation does not reset live progress; another pass never borrows the current pass's counts.
- 100% with further discussion is valid and writes no marks. A file-marking failure remains visible even with 100% coverage and does not enable Implement or claim successful completion.

### Jev and debug

- Missing/empty/whitespace key produces zero network requests and zero effective exclusions. All changes remain required.
- Significant/insignificant/uncertain and malformed/missing results follow the stated rules; only accepted insignificant results exclude exact targets.
- No silent truncation; an over-budget or mixed-context case remains required. Provider errors do not block UI or silently clear gaps.
- Cancellation, timeout, restart, duplicate calls, and late obsolete callbacks neither replay paid work nor mutate another pass.
- Debug expands without provider work, opens exact sides/ranges, retains historical revision references, and leaves answer focus/drafts intact.
- Require review is durable, wins against late insignificant results, and blocks conclusion only for uncovered portions. A successful answer can cover an overridden region normally.
- Reopening an active pass without a key disables effective cached exclusions; completed review receipts are not undone retroactively.
- Use adversarial fixtures such as a meaningful import-target change, removed permission check, changed default, and deleted assertion beside mechanical formatting. Inspect live errors honestly rather than asserting model outputs are deterministic.

### Baselines and recovery

- No file marks before a valid completion. Rejected or cancelled passes leave prior file marks unchanged.
- Marks use R even if code is now R'; Files shows the R-to-R' residual according to each existing backend's semantics.
- Crashes before the completion record, between path marks, after marking, and before MCP/UI acknowledgement recover honestly without duplicated history or newer-version credit.
- An old pending completion cannot overwrite a conflicting newer file mark; replay of completed receipts cannot undo a later manual unreview.
- Newer pass state, unrelated Files/Threads behavior, native agent binding, readable wakeups, edited Implement scope, and no automatic Implement replay all retain their existing regressions.
- Large geometry and supporting lists restore without source archives or repeated repository scans; ordinary diff-cache/grouping behavior remains intact.

## 12. Acceptance demonstration and stopping point

Use an isolated Git/jj fixture with three changed text files, a removed branch, one real metadata change, and unchanged supporting source. Include a mechanical candidate that can be excluded and a significant change initially left out of all questions.

1. Start without a key. Submit a question with one foreground snippet and several collapsed supporting ranges. Show 0% in the pinned header and the returned outstanding work. Answer to reach an intermediate percentage. Open the overview and demonstrate explored/partly explored/unexplored files, a jump to an old-side gap, and return to the question without losing selected options or unposted text. Reopen: the same ledger-derived coverage and view state survive, while file marks have not changed.
2. Try concluding early. Show a real error identifying the remaining significant/unclassified ranges, including the deleted side. Continue the same request with an appropriate question; do not lose the last answer.
3. Reach 100% through answered questions before concluding, with no file marks written yet; retain the ability to discuss an interaction further. Submit the valid conclusion and show its 100% header plus ordinary file marks at the reviewed version. The file overview remains inspectable after completion.
4. Modify only part of a marked file after the conclusion. Show the existing Files incremental diff. Do not implement fixes automatically to manufacture this demonstration.
5. In a fresh pass with a key and a harmless synthetic/source-sharing-appropriate fixture, show an actual Jev result, the small debug zone accessible from the coverage overview, and separate explored/excluded counts. Show an excluded-only file with an honest status rather than calling it explored. Record returned model and measurements; separate live observations from deterministic assertions.
6. Use Require review on an uncovered exclusion. Show the header and file group update consistently as it returns to outstanding work and blocks conclusion until an answered question covers it. Include deterministic zero-required, incomplete-inventory, and provider-failure demonstrations proving honest progress/fallback rather than a false 100%.
7. Demonstrate an interrupted local file-marking finalization with the deterministic harness. Reopening finishes or reports it without sending an agent/Implement prompt or crediting a later code version.

The handoff should state the exact code revision, actual checks, tested backend paths, live-model observations and limitations, and any deferred problems. Do not reuse earlier test counts or demos as proof of this slice.

**Done means:** answered questions persistently account for essential and supporting changed regions; a pinned coverage indicator and clickable file overview show that progress and navigate remaining gaps without disrupting the interview; the agent receives useful filtered gaps; Jev is optional and auditable; premature conclusions fail; accepted conclusions show 100% and set ordinary file baselines at the reviewed version; and subsequent changes use the existing incremental-diff machinery.

## Reference basis

Product policy comes from the latest user decisions in this conversation. The linked repository sources establish the implementation baseline, not endorsement of the new requirements.

- **[R1]** PR #3, head checked on 2026-09-21: <https://github.com/flupke/herdr-progressive-reviewer/pull/3>
- **[R2]** Question/answer, submission, and association types: <https://github.com/flupke/herdr-progressive-reviewer/blob/44ceca7049043594619a762610f8dec818ad68a1/crates/review-explore/src/interview.rs>
- **[R3]** Comparison preparation and current navigation mapping: <https://github.com/flupke/herdr-progressive-reviewer/blob/44ceca7049043594619a762610f8dec818ad68a1/crates/review-explore/src/capture.rs>
- **[R4]** MCP operations and response: <https://github.com/flupke/herdr-progressive-reviewer/blob/44ceca7049043594619a762610f8dec818ad68a1/crates/review-mcp/src/lib.rs>
- **[R5]** ReviewTracker, baseline grouping, diff cache, and current marking: <https://github.com/flupke/herdr-progressive-reviewer/blob/44ceca7049043594619a762610f8dec818ad68a1/crates/review-state/src/review.rs>
- **[R6]** Explicit-baseline file record storage: <https://github.com/flupke/herdr-progressive-reviewer/blob/44ceca7049043594619a762610f8dec818ad68a1/crates/review-store/src/checkpoint.rs>
- **[T1]** Official TypeSafe model/limit reference, checked 2026-09-21: <https://docs.typesafe.ai/models>
- **[T2]** Official typed questions and independent supplied-context evaluation: <https://docs.typesafe.ai/primitives>
- **[T3]** Official HTTP request/response reference: <https://docs.typesafe.ai/api>
- **[T4]** Official limitations relevant to narrow judgments and context selection: <https://docs.typesafe.ai/model-jaggedness/jev-1.13>
