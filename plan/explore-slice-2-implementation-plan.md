# Explore — slice 2: durable progress and safe resumption

Implement this as an additive slice over the shipped Explore experience. This is not a replacement for the first-slice plans: keep their latest overrides and change only the persistence and recovery behavior specified here.

**Goal:** closing or restarting the reviewer must not erase the investigation or accidentally repeat an action. Reopening restores the questions, answers, evolving agenda, conclusions, human edits, and reading position—not a newly generated questionnaire.

**Persist the investigation, not the repository.**

## 1. Starting point and source of truth

This guide is based on the supplied handoff, **“Explore: first-slice deltas for second-slice planning.”** It reports installed commit `a98fd9967d8c92d39c5e0b0ef6f387478db8a50b` in jj change `tqrxzpsnnqxrronpxsvnrrurmyvxnpyn`. These are reported endpoints, not a claim that the checkout still matches them.

Before implementation, follow the handoff’s reading order:

1. `AGENTS.md`, `CONTEXT.md`, and the current change description.
2. `plan/explore-milestone-1-implementation-plan.md`, then `plan/explore-milestone-1-ui-update.md`, including **Follow-up: review the working copy**.
3. `plan/explore-milestone-1-adaptive-interview-update.md`, including **all** subsequent follow-ups. Later transport, advancement, layout, and conclusion rules supersede earlier ones.
4. `docs/usage.md` → **Explore a change (experimental)**, `docs/mcp.md`, `crates/review-explore-runner/src/interview.md`, and `docs/explore-evidence-mcp-update.md`.

Recheck the checkout, then create the fresh feature change required by `AGENTS.md`. When examining amended first-slice revisions, compare explicit endpoints with `jj --ignore-working-copy diff --from <base> --to <tip> --git`; do not substitute a merge-base comparison. Do not treat the original first-slice fixed point as the base for the new feature.

The supplied handoff documents what shipped and leaves second-slice choices open. **The recovery policies below are new requirements for this slice**, not claims about existing behavior. Inspect the actual code before selecting serialization or transport changes.

## 2. Scope and preserved contracts

Implement durable Explore state, automatic local restoration, and safe handling of interrupted work. Keep the existing interview behavior and UI.

| Preserve | Do not reintroduce or add |
| --- | --- |
| Working-copy source access; base-side evidence from repository history; the assumption that source remains unchanged during a review pass. | Repository snapshots, eager source capture, source hashing/freshness gates, capture limits, or freshness pauses. These were removed, not deferred. |
| The adaptive agenda, direct responses to reviewer questions, dependencies/order, attributed retirement, supersession and reconsideration, and separate reversibility/blast-radius assessments. | A static regenerated questionnaire or a new agenda model that discards those semantics. |
| One question at a time, Opening/history navigation, default keyboard choice selection, supplemental text, and None of the above. | A return to the earlier sidebar layout or an always-expanded conversation feed. |
| Curated primary evidence, supporting sources, explanations of what each snippet establishes, native diff/source views, yellow outlines and resizing. | A research catalog presented as the reviewer’s reading assignment. |
| Explore MCP methods `submit_question` and `submit_conclusion`, and the existing pinned-conversation prompt flow. | `get_explore`, `read_explore`, `get_explore_answer`, source-registration tools, mailbox/file transport fallbacks, or a second sender. |
| Distinct conclusions with `summary`, `to_be_implemented`, `future_work`, independently edited task/reply fields, and an explicit Implement action. | Structured task-completion tracking, automatic implementation, or an implement/review loop. |
| Existing Files baselines and file-level review marks. | Marking Files reviewed because a question was answered, a conclusion was posted, or history was restored. |

Also defer classifiers, coverage/baseline reconciliation, new closing cross-area checks, cross-source-generation reasoning, and selective revalidation. This slice makes the existing investigation durable; it does not broaden its review guarantees.

## 3. Persist domain state, not UI objects

Use the repository’s existing storage conventions. Introduce a versioned durable representation of the current domain types, reusing shared types rather than copying them into a competing model. A general event-sourcing framework is not required.

Give each pass a durable identity scoped to the canonical checkout and existing logical review identity. Reuse existing pass/request/question identifiers when suitable. IDs must survive reopening; a reused pane or the same repository path alone is not an agent identity.

Persist these independently meaningful records:

| Record | Required contents |
| --- | --- |
| Pass | Stable identity, logical review binding, Opening content, existing lifecycle state, schema/storage revision, and pinned native agent-conversation identity when known. |
| Displayed history | Exact question versions, options and outcome meanings, direct agent answers, and distinct conclusions, in their original order. Preserve immutable displayed content. |
| Reviewer contributions | Exact selected option ID **and text/outcome**, optional comment, free-text questions, corrections and their original targets. Do not reconstruct old answers from newer options. |
| Investigation | The current agenda plus the changes needed to explain its evolution: dependencies/order, retirements, supersessions, reconsiderations, reasons and attribution. Preserve original decisions when new information challenges them. |
| Evidence | Existing references, sides, ranges, primary/supporting roles, explanatory labels and historical revision locators where already available. Do not serialize live source buffers or capture the repository. |
| Editors | Unposted answers/comments by exact question version; each conclusion’s edited task text and its separate reply draft; choice selection and relevant editor positions. |
| Delivery | Logical turn/request identities, origin and destination binding, exact submitted payload, acknowledgement/idempotency information, and what is actually known about dispatch. |
| Implementation request | Originating conclusion, the exact human-edited task scope authorized by the Implement click, pinned conversation, logical request ID and dispatch status. Keep this separate from later edits to the task box. |
| Reading position | Selected history entry, current evidence reference, outer/code scroll positions, manually chosen viewer height versus Fit evidence, and editor/focus state needed to resume naturally. |

Keep task and future-work fields as strings. A delivery record needs an identity; individual tasks do not need new IDs or completion states.

Do not persist bearer tokens, live connections, process handles, component instances, watcher state, syntax caches, or LSP objects. Reestablish runtime capabilities through the existing mechanism.

### History must remain structurally correct

The sequence below must survive round-trip storage without collapsing entries or sharing editor state:

```text
Question 1 -> exact answer -> correction
           -> agenda branch retired, with reason
           -> Conclusion 1 [task edits A, reply draft A]
           -> reviewer follow-up -> Question 2 -> answer
           -> Conclusion 2 [task edits B, reply draft B]
```

Only the current conclusion remains eligible for Implement under the existing rules. Browsing an older conclusion must not reactivate its implementation controls.

Starting an explicit New pass must not overwrite the previous pass. Retain older records and provide a minimal way to open them as history; reuse existing navigation rather than building a history dashboard. Do not automatically carry conclusions into the new pass.

## 4. Make acknowledged changes durable

Saving cannot depend on a graceful close.

For posted reviewer contributions, accepted MCP submissions, agenda/decision changes and implementation authorization, make the domain update and its deduplication information durable before reporting success or performing the dependent external action.

The intended inbound boundary is:

```text
MCP submission
  -> validate identity and payload against latest state
  -> atomically persist accepted domain change + idempotency record
  -> apply/publish the corresponding UI update
  -> return success
```

Preserve the current rule that MCP succeeds only after validation and application. Durability adds a requirement; it does not replace application acknowledgement. If storage succeeds but acknowledgement or UI publication is interrupted, recovery/retry must finish from that committed state, not append another question or conclusion.

An identical accepted retry must not append history, consume an answer, advance twice, or move the cursor again. A conflicting payload using an existing identity must not overwrite the original. Keep semantic request identity distinct from renewable transport credentials.

Before posting an answer, durably bind it to the displayed question version and preserve its exact content. If saving fails, leave the user’s text available and do not send a prompt for an unsaved answer.

Use atomic storage and the flushes needed for the stated crash guarantees. Reuse existing locks and helpers where they provide those guarantees. Do not allow a stale whole-pass write from one window to erase another window’s accepted update. Apply mutations against the latest stored revision; retain conflicting unposted edits rather than silently overwriting them.

### Autosave without a Save button

Save editor changes automatically through the existing event/worker architecture; coalescing rapidly repeated edits is acceptable. Flush pending editor changes on a normal close and before an action depends on them. Keep disk work off the rendering path.

An abrupt process death may lose editor changes that have not yet completed persistence. It must not lose an acknowledged post, MCP response, or implementation authorization. Document and test that distinction rather than promising that every keystroke is crash-proof.

Reading position is lower priority than domain state: restore it best-effort and clamp it to the current layout. Never let a position write replace newer conversation content.

Do not add periodic filesystem polling. Reuse filesystem events where external state changes need observation.

## 5. Restore locally; continue with the existing agent conversation

Opening the reviewer restores the most recent pass for the selected logical review. Restore the previously selected entry, evidence, task edits and unposted text automatically, without a routine Resume/Accept dialog.

**Restoring is not a new agent turn.** Do not regenerate the map, replay history as fresh events, post drafts, reset an answer to the default choice, or send a new kickoff simply because Explore opened.

| Situation on reopen | Required behavior |
| --- | --- |
| No saved pass | Existing first-use experience. Do not invent history. |
| Saved pass, same native agent conversation | Restore locally. The next normal reviewer action continues through the existing prompt pipeline without a separate approval step. |
| Agent not currently available, or identity not yet reported | Show saved history and preserve edits. Defer continuation until the binding can be established; do not guess or treat late identity discovery as replacement. |
| A genuinely different native conversation | Keep history available, reject stale work, and explain the mismatch briefly. The user can return to/resume the original conversation or explicitly start a New pass with the selected agent. Do not silently rebind. |
| Interrupted agent turn | Restore its actual state and offer the existing Retry/Cancel behavior as appropriate. Reopening alone does not resend it. |
| Saved conclusion or implementation request | Restore each field and its actual dispatch state; do not turn the conclusion into an implementation command. |

Continuation is scoped to the **same native agent conversation**, including a resumed instance when the existing identity mechanism can verify it. Restoring a pane at the same path is insufficient. If an unresolved original binding cannot safely be proven, do not send.

Automatic reconstruction of lost model context in a different agent conversation is outside this slice. Do not solve that by restoring removed context-fetch tools, adding a history file fallback, or pretending the new conversation remembers the old one. Local history remains useful even when its original agent is unavailable.

Reestablish runtime authentication normally. Do not make old access tokens durable or accept calls solely because a pass ID matches. Preserve the shipped distinction between learning an initially unknown native session ID and an actual replacement.

## 6. Recover delivery without silently repeating work

Extend the **shared delivery pipeline**, including its readiness checks, cancellation, replacement and pinned conversation behavior. Do not create an Explore-only durable sender. Keep thread and Guide behavior unchanged unless a shared reliability fix requires a narrowly tested change.

For this slice, **pending Explore work is restored, not automatically resent on reopen**. When the user explicitly retries, continue using the normal readiness pipeline; a busy/focused agent or unposted terminal input must still postpone delivery.

### Interview turns

A stored answer remains posted even when its subsequent agent wakeup was interrupted. It must not revert to unposted text or require the reviewer to answer again.

An explicit Retry should reuse the logical turn identity and exact original answer payload, check for an already committed response first, and follow current retry semantics. Describe uncertain delivery as uncertain. Do not append the same reviewer contribution to history again.

If a valid response arrives through a properly rebound current connection, handle it through the normal durable acceptance path. Late work from a cancelled, replaced or different pass/conversation must not enter the active pass.

Preserve shipped live advancement: a newly accepted question becomes selected despite intervening input, while preserving the previous draft and leaving Files/Threads active when appropriate. Local restoration and duplicate responses are **not** newly accepted questions and must not trigger that navigation.

### Implement is an external action, not a recoverable UI event

The Implement click authorizes the exact edited task text and its validation, as today. Only that task scope is sent—not the conclusion summary, unedited generated task text, future work, or all unresolved agenda items.

Persist that authorized payload before queuing it. Later editing must not mutate an already authorized request. A deliberate new Implement action uses the current text and existing eligibility rules; it must not accidentally reuse the old authorization with different content.

Model dispatch knowledge explicitly, reusing or extending current types:

```text
Authorized / queued, definitely not attempted
  -> persist "dispatch may begin"
  -> attempt external delivery
       -> authoritative success: Delivered
       -> authoritative no-send result: Not sent / Cancelled
       -> crash or ambiguous outcome: Delivery unknown
```

The durable transition before the external attempt is essential: a crash after submission but before saving its result must not recover as “definitely not sent.” Reconcile cancellation with the authoritative delivery result; cancellation requested is not proof of cancellation completed.

| Recovered implementation state | Required behavior |
| --- | --- |
| Definitely queued, never attempted | Keep it paused. An explicit existing Implement/retry action may submit the recorded scope. No automatic replay. |
| Definitely cancelled or failed before sending | Show the real status. A further submission requires an explicit user action. |
| Delivered | Retain the delivery record and suppress duplicate sending. This is **not** evidence that tasks were implemented or validated. |
| Attempt outcome unknown | Show “Delivery outcome unknown” with concise guidance to check the original agent conversation. Do not automatically retry or offer a routine retry that assumes non-delivery. |

Do not claim exactly-once external delivery if the transport cannot prove it. At this boundary the guarantee is durable intent, honest outcomes, and no automatic repeat. A separate implementation request after an ambiguous attempt must be a deliberate user action, not restart recovery.

Persisting an authorization does not authorize a new scope, a new agent conversation, a new review pass, or an automatic implement/review cycle.

## 7. Working-copy evidence and pass boundaries

Restored evidence opens through the same working-copy/history access used today. Persist locators and explanations, not an archive of source bytes. Do not hash or scan the repository on reopening to decide whether restoration is allowed.

The source-unchanged assumption now also applies when continuing a saved pass. Saved answers and conclusions are records of that investigation, not proof that subsequently modified code has been reviewed. State this limitation once in the usage/recovery documentation; do not restore the removed permanent warning rows.

If the user knows the source has changed, use the existing explicit New pass flow. Preserve the previous investigation as history without automatically reconciling it with new source, marking its tasks complete, or carrying its accepted conclusions forward.

A missing historical revision, file, or range must yield a local unavailable-evidence indication. Preserve the question and discussion, allow other evidence to open, and never substitute a different source while labelling it as the old reference. Working-copy references remain labelled working copy; reopening does not make them historical snapshots.

If implementation was delivered, restore that fact. Do not infer completion or start a new comparison automatically. Review-after-implementation policy and task satisfaction remain later work.

## 8. Recovery UX and storage failures

Keep recovery almost invisible when it succeeds: the same question or conclusion, the same edits, and the next normal action. No recovery wizard, forced recap, routine confirmation, or new persistent toolbar.

Keep independent ownership of task and reply editors, including their focus. Restoring history must not activate a default answer or submit the content of the wrong editor. Preserve manual evidence heights when practical and refit/clamp safely after terminal resizing.

Report storage errors locally and precisely. Corrupt, oversized or unsupported data must not appear as an empty new pass or overwrite recoverable history. Preserve the original files; stop unsafe mutations to the affected pass while keeping Files/Threads and readable history usable where possible.

Use a versioned Explore storage namespace. Fresh installations have no persistent Explore history to migrate; opening them must still work normally. Do not migrate or alter unrelated thread/file-review formats just to add Explore.

Apply reasonable resource bounds without treating the maximum size of one MCP response as the maximum size of an entire growing interview. Test a long pass with many individually valid entries. Avoid per-keystroke recompression of large history or source data.

## 9. Implementation sequence and entry points

Deliver this as one scoped feature, in small testable increments under the current repository workflow.

**First: durable domain round-trip.** Inspect the existing interview, conclusion and editor-ownership types. Add serialization/storage and command-level tests for a realistic evolving pass. Prove that original questions, corrections, agenda reasons, separate conclusions and human edits survive before integrating startup.

**Second: local save/restore.** Wire domain changes, autosave and view restoration. Demonstrate close/reopen with zero Explore-originated prompts and no Files marks changed. Include missing agent, unavailable source and invalid-store behavior.

**Third: durable acceptance and delivery recovery.** Integrate persistence with MCP acceptance and the existing shared dispatcher. Exercise interruption points, idempotent retries, session rebinding and implementation dispatch uncertainty. Do not ship persistence that forgets pending implementation state.

**Fourth: end-to-end validation and documentation.** Run isolated restart/race tests, a fresh same-conversation real-agent acceptance check, required reviews, and installation. Update usage, MCP/recovery notes and the handoff with actual validation evidence and limits.

Starting points from the handoff; verify their current ownership before editing:

| Area | Entry points |
| --- | --- |
| Domain state | `crates/review-explore/src/`, particularly `conclusion.rs` and the current interview/agenda types. |
| Existing storage integration | Inspect `crates/review-store/` and its locking/atomic-write helpers; extend narrowly. |
| Shared delivery and binding | `crates/review-thread-service/src/delivery.rs`, `pinned_agent.rs`. |
| MCP validation/application acknowledgement | `crates/review-mcp/src/handler.rs`. |
| Explore runtime and implementation dispatch | `crates/reviewer/src/runtime/explore/`, particularly `implementation.rs`, and adjacent tests. |
| History, editors and restoration | `crates/components/explore/src/`, particularly `conclusion.rs`, plus the current runtime/UI tests. |
| Agent prompt contract | `crates/review-explore-runner/src/interview.md`; amend only as needed for explicit interrupted-turn recovery through the existing protocol. |

## 10. Required tests and acceptance evidence

Use deterministic agents with the real isolated Herdr/MCP infrastructure for failure and race tests. Never use or restart live user panes.

| Case | Required result |
| --- | --- |
| Adaptive history round-trip | Exact answers, corrections, original decisions, retired branches/reasons, dependencies and risk assessments survive; retired does not become accepted. |
| Multiple conclusions | Conclusion → question → conclusion remains ordered, with independent edited task text/reply drafts and only the current conclusion eligible to Implement. |
| Ordinary close/reopen | Restores entry, selection, drafts and evidence position; sends no Explore prompt and marks no file reviewed. |
| Accepted output, lost acknowledgement | Restart and identical MCP retry result in one durable entry and no duplicate navigation. A conflicting retry cannot rewrite it. |
| Posted answer, interrupted wakeup | Answer stays posted; explicit retry keeps its exact option/comment and logical turn identity. No automatic resend. |
| Implementation crash boundaries | Queued-before-attempt, during attempt and delivered-before-result-save restore distinctly; ambiguous attempts never become “not sent.” |
| Implementation scope | The task scope is exactly the text authorized by that click; existing binding/validation instructions remain. Later edits, future work and summary text cannot expand that scope. |
| Cancel/send race | Status follows authoritative dispatch knowledge; no false cancellation or duplicate resend. |
| Conversation identity | Same native conversation resumes; late initial ID discovery is not replacement; genuinely different conversation and stale grants are rejected. |
| UI/advancement regressions | Live accepted questions follow the current advancement rule; restoration/retries do not. Files/Threads stay active when required and drafts remain attached to their owners. |
| Persistence failure/concurrent update | No false success, silent history loss or stale full-pass overwrite; unposted text remains available. |
| Working-copy boundary | Reopening uses existing source/history access without repository capture or freshness scans; unavailable evidence does not destroy history. |
| New pass and long history | Previous pass remains accessible; no automatic decision carry-forward; many valid turns do not hit a single-response-sized history cap. |

### Human acceptance demonstration

In an isolated fixture, complete a useful adaptive exchange, including an answer that retires a branch. Reach a conclusion, edit its task scope, and leave a separate reply draft. Close and reopen the reviewer while retaining the same native agent conversation.

Demonstrate that the exact agenda/history, conclusion edits and reply draft return without an agent wakeup. Ask a normal follow-up and show that the same agent conversation continues rather than starting a new questionnaire. Navigate between the earlier and later conclusions and verify editor ownership.

Separately, demonstrate interrupted Implement delivery with deterministic fixtures: a definitely queued request stays paused, a delivered request is not repeated, and an ambiguous crash is honestly labelled unknown. Do not intentionally create ambiguous real-world implementation side effects for a demo.

The handoff says the archived live-model demo predates the final MCP/conclusion changes. Do not reuse it as proof of current resumption. Record the new evidence and distinguish real-agent observations from deterministic tests. If a real-agent check cannot run, report that limitation rather than claiming acceptance.

Run the checks, reviews, description and installation required by current `AGENTS.md`. The handoff’s last reported full-check command was `NEXTEST_TEST_THREADS=8 make check`; use the current repository instructions as authoritative. Follow its private-socket escalation rules and retain test isolation.

## Completion and stopping point

Slice 2 is complete when a real investigation can be closed and resumed with its original context, edits and next action intact, and interrupted delivery cannot silently erase or repeat work.

Finish with a concise implementation handoff: what persists, how to resume, same-conversation limitation, working-copy assumption, interrupted Implement behavior, tests/demos performed, and any remaining failures.

**Stop here.** Do not automatically proceed to Jev, task completion tracking, coverage reconciliation, selective revalidation or review-after-implementation automation.
