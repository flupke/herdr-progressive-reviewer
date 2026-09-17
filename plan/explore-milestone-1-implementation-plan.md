# Explore — Milestone 1 implementation plan

**Deliverable:** a real, question-first interview inside Explore, beside the existing fully functional code view.

**Build only this milestone, demonstrate it, and stop for product feedback.** Persistence and recovery remain requirements for finished v1, but are not part of this milestone. This document replaces the original plan’s fixture-first milestone 1 and its incompatible interaction requirements; it does not authorize implementation of the rest of v1.

## 1. What we are testing

Can the reviewer understand and judge a consequential part of a change through a concise conversation, without first reading its surrounding files in order?

The working loop is:

```text
Start Explore on a frozen change
  -> agent presents one concise question
  -> code view opens its relevant evidence, outlined in yellow
  -> reviewer investigates and answers inside Explore
  -> answer is recorded; pending questions evolve
  -> next question opens its evidence
```

This must work with the existing connected implementation agent. Deterministic fixtures are necessary for automated tests, but a fixture-only demonstration does not complete the milestone.

### Decisions already made

| Requirement | Consequence for this milestone |
| --- | --- |
| Small questions, not explanatory reports | Lead with one question and concise alternatives; expand rationale and discussion on demand. |
| Review first, fix afterwards | Collect required changes as follow-ups. Do not implement fixes during the interview. |
| Agent maintains pending questions | Allow additions and refinements without approval for each agenda update. |
| Explicit answers record decisions | No second Accept click. Preserve the exact answer and its attribution. |
| Answer inside Explore | The normal loop must not require switching to the agent terminal. |
| Evidence follows question changes | Activate the relevant file/ranges when intentionally advancing to a question. |
| Fully functional code view | Reuse the existing diff/source view, including LSP, search, selection, and comments. |
| Yellow boxes identify relevant code | Outline the active question’s ranges; this is neither risk scoring nor review progress. |
| Keep file-level review marks | An answer, displayed snippet, or completed interview never marks a file reviewed. |
| Human inspection still required | Files remains the workflow for inspecting remaining changes. No classifier or automatic skipping. |

## 2. Scope and explicit limitations

Implement a single active exploration per open reviewer, with a small in-memory question map, transcript, answers, follow-ups, and selected evidence. Switching to Files or Threads and back must preserve this state while the reviewer remains open.

Show a visible experimental limitation: **“Explore progress is not restored after closing the reviewer.”** Temporary request files are transport artifacts, not a recovery system. Existing ordinary review threads retain their existing persistence behavior.

Do not add Explore storage migrations, crash recovery, generation-history browsing, selective revalidation, hunk-level review marks, TypeSafe/Jev integration, automatic tests or fixes initiated by Explore, a graphical decision tree, or a new model-provider configuration. Do not duplicate the code renderer or construct a general-purpose agent orchestration framework.

Retain essential safety boundaries now: exact source identity, response validation, cancellation, explicit stale/unknown state, and attribution of human answers. Deferring persistence does not defer these boundaries.

## 3. Start with an exact, bounded review context

### Capture the full selected change

Use the repository’s complete base-to-checkpoint comparison, not a mixture of per-file “since last reviewed” diffs. Include changed files regardless of Files filters or review marks. Preserve existing Files semantics; do not reset marks or redefine its baselines.

Reuse repository snapshot and frozen-source facilities. Keep exact before/after source available for the lifetime of this exploration. Supporting unchanged callers and tests must also resolve against that checkpoint, not whatever is on disk when a link is opened.

Create a deterministic manifest of changed files, textual hunks, and non-text changes. Give entries request-local IDs and retain lossless path identity. Missing or unreadable content stays visible as a limitation. Do not silently omit binary, rename-only, deletion, mode, or other supported change kinds.

The agent associates topics with manifest entries. The application exposes unassigned entries as **not yet mapped**. These associations are navigation/accounting data, not proof of inspection or completeness. A collapsed map is sufficient; do not build a coverage dashboard.

### Remain on one frozen pass

Use the existing filesystem-event pipeline to detect source changes. After a possible change, freshness is `Unknown` until checked. If the captured comparison changes, the exploration is `Stale`, including when an unchanged caller outside the selected evidence changed.

Keep frozen evidence and existing answers readable, but suspend new current-checkpoint decisions and further interview advancement until freshness is known. If stale, offer an explicit new pass with a warning that this milestone will not carry its in-memory progress across. Never transfer decisions automatically.

File marks, UI filters, thread messages, and agent response files are not source changes. Do not introduce periodic repository scanning.

## 4. Implement the smallest structured interview contract

Use a small domain module or crate with types and methods, separate from rendering. Names below describe proposed responsibilities, not pre-existing APIs.

| Object | Minimum responsibility |
| --- | --- |
| `Exploration` | Instance ID, review/comparison identity, checkpoint, manifest, topic map, transcript, follow-ups, and freshness. |
| `Question` | Stable ID and version, parent topic, one concise question, optional consequence/visual, alternatives, and evidence IDs. |
| `EvidenceRef` | Snapshot source identity, old/new side, validated line ranges or non-text target, and a short relationship label. |
| `ReviewerAnswer` | Unique answer ID, exact question version and checkpoint, selected option identity/text and/or verbatim free text. |
| `InterviewUpdate` | Request identity, optional interpretation of that answer, pending-topic additions/refinements, and a proposed next question or closing state. |

Question content and evidence shown to the reviewer must be immutable for that answer. Pending questions can evolve; changing their wording later must not change what a previous answer meant. For this milestone, support adding and updating pending topics rather than implementing topic deletion, merging, or rewriting settled conclusions.

### Record answers without a confirmation funnel

Submitting a choice or free-text answer records it immediately in the open exploration. Retain the literal answer as authoritative. Selecting an option must send its stable identity and wording, not an unbound “A” or “B”.

Distinguish `Open`, `Accepted`, `NeedsFollowUp`, and `Deferred`. A domain-context answer does not necessarily settle a topic. Conditional agreement or a requested change stays `NeedsFollowUp`; ambiguous answers stay open and receive a concise clarification. Silence, viewing evidence, and ordinary navigation are not acceptance.

The agent may interpret an answer and propose a conclusion, but only for the exact answer/question/checkpoint supplied in that turn. It may not create an answer, revise an earlier accepted decision, or mark files reviewed. Show a short recap such as **“Recorded: keep resolved; add a regression test — follow-up.”** Let the reviewer correct that interpretation through the same answer interface, without another routine approval step. Corrections append to history rather than replacing the original answer.

Structural validation can establish attribution, not prove that free text was interpreted correctly. Preserve the exact answer, keep the interpretation visible and correctable, and evaluate this behavior in the real-agent demo. Do not claim a semantic guarantee that the application cannot enforce.

### Keep the agent transport small

Working implementation choice: use an Explore-specific, per-turn structured request/response mailbox, following the existing guide-delivery pattern. Reuse existing selected-agent resolution, safe prompt delivery, cancellation, and filesystem notifications wherever applicable. Share transport helpers where useful; do not change the author’s-guide contract.

Use the existing implementation-agent conversation, not a new model session or one fresh subagent per answer. Maintain a lightweight interview turn log in Explore; ordinary source comments continue through the existing thread machinery. Do not force every question into a new ordinary code thread or modify public MCP schemas just to deliver this milestone.

Each request contains an instance ID, turn/request ID, checkpoint, current question version, the submitted answer if any, the relevant transcript/recorded decisions, and access to the frozen evidence and change manifest. Use private request directories, bounded reads, temporary-file-plus-atomic-rename publication, and event-driven response watching. Keep token/access handling and client permissions within existing project conventions.

Accept at most one result for the outstanding request. Repeated delivery is idempotent. Wrong-checkpoint, superseded, cancelled, or previous-process results cannot change the active interview. Invalid payloads retain the last usable question and answer, show a useful error, and permit explicit retry. Do not silently truncate evidence or retry forever.

Validate identities, known answer references, source membership, sides, ranges, and state transitions in application code. Resolve source references through the frozen repository namespace; model output must not authorize arbitrary filesystem reads, path traversal, shell commands, or repository edits.

### Give the agent a self-contained interview prompt

Check in an Explore-specific prompt rather than relying on personal `/grilling` or `/show-me` installations. It should direct the agent to scan the full change, form a brief provisional behavioral map, and ask the most consequential question whose prerequisites are understood. If context is unavailable or the scan is incomplete, retain that limitation explicitly.

Present one question at a time. Use concise, genuinely different options, with a recommendation and its reason where a recommendation is justified, and always allow free text. Gather only human context that changes the review; look up source facts instead of quizzing the reviewer.

Keep detailed reasoning collapsible. For a complex relationship, produce the smallest useful terminal-readable sequence, call tree, pseudocode, or diff sketch beside the question, labelled simplified or proposed as appropriate. A new HTML renderer is unnecessary for this milestone.

Keep implementation, stated intent, and inferred rationale distinct. Include consequential choices even when code appears correct. Separate straightforward defect findings and required fixes from unresolved design questions. Referencing a test does not establish that it ran or passed.

Use the reviewer’s answers to refine pending questions without requesting agenda approval. Do not edit source during this frozen review pass. Do not announce review completion merely because the current topic list is exhausted.

## 5. Build Explore around the existing code view

### Layout and input

Add Explore beside Files and Threads. On a normal-width terminal, show a concise question/answer pane beside the existing code pane. Use a compact stacked/focusable layout on narrow terminals rather than squeezing both into unreadable columns.

Keep the question, alternatives, and answer composer immediately visible. Place the map, prior discussion, detailed rationale, findings, and follow-ups behind compact expandable controls. Reuse existing editor and focus conventions. Support keyboard and mouse; audit shortcut conflicts rather than assigning speculative global keys.

Minimum actions are Start, Send, Defer, evidence selection, previous/next question navigation, map/history expansion, and Cancel/Retry for pending agent work. Provide loading, unavailable-agent, invalid-response, stale, and unknown-freshness states without clearing the reviewer’s text.

Sending an answer normally advances to the next returned question without an Accept click. Do not steal focus if the reviewer started typing, browsing code, or revisiting history while the response was pending: retain the response as ready and provide an explicit next-question action. Pending-map updates and ordinary replies never trigger a viewport jump by themselves.

### Reuse capabilities, not just appearance

Mount/reuse the actual diff/source component and its navigation services. Preserve syntax highlighting, scrolling, code selection, search, ordinary review comments, and LSP operations. Do not substitute a snippet-only panel with a similar visual style.

The question and unfinished answer stay in place while the reviewer follows a definition, opens a test, or browses another file. Give a simple way back to the question’s primary evidence. This is not another full repository browser: Files remains available for that workflow.

### Yellow evidence outlines

Add a transient decoration layer keyed by active question and exact source identity. Outline each relevant range in yellow. Support multiple disjoint ranges in one file and evidence in several files; only decorate the source currently displayed.

Map ranges through the existing old/new diff-line model and rendered wrapping. Do not widen an outline to include unrelated lines between separate ranges. Handle deletion-only evidence and clipped ranges honestly; off-screen continuation must not look like an exact range endpoint. Non-text evidence gets a labelled file-level reference, not a fictitious line box.

Keep selection, cursor, comment markers, search matches, and existing diff information usable. The outline indicates **“relevant to this question”**, never severity, acceptance, or human inspection. Update outlines on intentional question/evidence changes without resetting the composer.

### LSP integrity: the bounded milestone approach

Verify the current code view’s actual LSP behavior before adapting it. Do not assume it already supports historical indexing.

For this milestone, retain normal LSP on the current/new side while the live repository matches the frozen checkpoint. Map navigation results back to the exact source view, with provenance checks before treating a destination as checkpoint evidence. Ensure delayed LSP responses cannot silently navigate to newer code after a source change.

Do not send old-side/deleted-line coordinates to a new-side document as though they matched. Reuse any valid existing mapping; otherwise make that particular operation’s limitation explicit. External dependency destinations must be labelled external/current, not silently included in frozen evidence.

Do not build a separate historical language-server workspace for milestone 1. If source diverges or freshness cannot be established, retain the frozen view and block or explicitly separate live navigation. **Normal new-side LSP must actually work in the unchanged-checkpoint demo; disabling LSP throughout Explore is not an acceptable shortcut.** Document any old-side limitations rather than claiming full historical LSP support.

## 6. Implementation sequence

These are steps within this milestone, not additional product milestones. Use the repository’s small-feature workflow for the change as a whole.

### Step 1 — Inspect integration points and define the thin contract

Read current `AGENTS.md` and `CONTEXT.md`, then inspect the UI component/event boundaries, full-change snapshot APIs, code-view source loading, LSP coordinate handling, and selected-agent delivery. Identify the smallest reusable boundaries before moving shared types.

Define the in-memory objects, turn identity checks, and frozen-source descriptors. Add reducer/validation tests. Do not begin with Explore persistence or a generalized transport refactor.

### Step 2 — Connect a real question/answer loop immediately

Use the selected implementation agent and a frozen small change to produce a structured question in Explore. Post a reviewer answer and receive the next question in the same agent conversation. Keep diagnostics and retry visible.

This is the first working integration target. It need not yet have every outline-rendering edge case, but it must be real interaction rather than a static topic fixture. Add deterministic transport/response fixtures alongside it for CI.

### Step 3 — Complete code-view integration and yellow outlines

Wire primary-evidence navigation, cross-file evidence selection, full code-view interactions, LSP provenance, and range decorations. Preserve each view’s navigation and unposted text. Test wrapping, scrolling, and old/new source mapping.

### Step 4 — Complete adaptation, answers, and safety behavior

Allow the agent to add/refine pending questions, expose the collapsed map and follow-up list, record attributed answers with concise corrections, and implement ordinary advancement without unsolicited jumps. Add stale/unknown handling and late-result rejection.

Keep file-review marks and existing thread resolution independent. Exhausting questions should direct the reviewer back to Files for remaining inspection, not report the code approved.

### Step 5 — Demonstrate, document, and stop

Run focused tests and required repository checks. Demonstrate the real loop and record what was actually observed, including LSP behavior and temporary-state limitations. Update usage/help with the experimental entry point. Follow required review/install steps from the current repository instructions, then stop for user feedback before starting persistence or other later work.

### Integration guide

These locations come from the previously inspected baseline, not a fresh audit of current HEAD. Verify names and dependency directions in the implementing checkout.

| Area | Expected work |
| --- | --- |
| `crates/review-ui`, `ui-actions`, `ui-events`, `ui-shortcuts` | Explore navigation, focus, actions/events, loading states, and help. |
| `crates/components/diff`, locations/navigation components, `crates/review-lsp` | Reusable frozen-source view, evidence decorations, and valid LSP navigation. |
| New small Explore domain/component modules or crates | In-memory interview state, validation, reducers, and question UI. |
| `crates/review-repository`, applicable `review-guide` source types | Full comparison and exact evidence access without duplicate snapshot/anchor types. |
| `crates/review-guide-runner`, Herdr delivery/runtime infrastructure | Reusable per-turn delivery primitives with an independent Explore prompt/schema. |
| Existing comment editor/thread infrastructure | Editor behavior and ordinary inline discussions, without changing their persistence/acknowledgement rules. |
| `crates/reviewer/src/runtime*` | Coordinate capture, turn lifecycle, selected agent, freshness, and component events. |

No Explore store migration is required for milestone 1.

## 7. Tests and acceptance

Automated tests must use deterministic agent outputs and the existing isolated Herdr test infrastructure where integration requires it. No model API access in CI. Use private test repositories/server/socket/config/state; never change or restart the user’s live Herdr server as a test fixture.

| Test area | Required behavior |
| --- | --- |
| Interview turn | Start -> question -> explicit answer -> next question; no duplicate submission or extra Accept action. |
| Adaptation | New domain context adds/refines pending questions without confirmation; displayed/answered versions and recorded decisions are not rewritten. |
| Answer attribution | Choice IDs survive reordering; free text remains verbatim; interpretations reference the exact answer; corrections append; scripted conditional cases remain follow-ups. |
| Evidence | Two changed files plus an unchanged caller/test open from the exact checkpoint; invalid/missing sources remain gaps. |
| Outline rendering | Disjoint, wrapped, partially off-screen, and old/new ranges decorate only intended code and do not break selection/comments/search. |
| LSP | A real configured language server supports normal new-side navigation; stale/delayed results and deleted-side coordinates cannot masquerade as frozen evidence. |
| Focus and position | Switching tabs or following definitions preserves the question/composer; background results do not interrupt newer user activity. |
| Existing review semantics | Questions, answers, outlines, and map updates never mark files reviewed or resolve ordinary review threads. |
| Failure handling | Missing agent, bad JSON, invalid IDs/ranges, unsafe paths, oversized/partial output, cancellation, duplicate and late responses are visible and recoverable without applying a wrong turn. |
| Freshness | Editing a different caller/configuration invalidates the pass; unknown freshness suspends decisions; review-mark/transport changes do not invalidate it. |

Exercise snapshot capture and stale handling for both Git and jj. Add focused real-language-server checks using the project’s existing test conventions. Report checks not run and any missing test prerequisites honestly.

### Real-agent acceptance demonstration

Use a small disposable multi-file change with a genuine policy choice, a related caller, and a test. Keep the reviewed code unchanged during the interview.

Start Explore and obtain a concise real-agent question. Confirm its primary evidence opens automatically and the relevant ranges are outlined in yellow. Follow a definition with LSP, search or inspect another supporting file, then return without losing the question or unposted answer.

Answer with domain context that should alter the pending questions. Observe that the agent incorporates it without asking permission to update the map. Record an explicit choice and then a conditional decision; confirm no second Accept click and that the condition remains a follow-up. Correct an interpretation once and check that the original answer remains visible.

Proceed to the next question and verify the evidence follows. Switch to Files and Threads and back. Confirm that ordinary file marks remain untouched and further human inspection is still required.

Finally, make an external source edit in the disposable checkout to test the stale guard. Frozen evidence must remain identifiable and no new conclusion may be presented as applying to the changed code. Do not require restart/resumption to pass this milestone: the UI must disclose that limitation instead.

The demo succeeds when the reviewer can exercise a meaningful judgment in the real loop, not merely when a plausible topic list renders. Present the working behavior and stop for feedback; do not silently proceed to the rest of v1.

## 8. Provenance and intentionally open work

This handoff incorporates the explicit decisions recorded in `explore-plan-review.md`: R0, R1, R2.1, R4.1, R3.1, R4.2, R3.2, R6.1, R4.3, and R4.4. It supersedes the original immutable-topic, mandatory-Accept, and fixture-first requirements where they conflict.

The earlier repository inspection was pinned to `3f84ddb537afd758c1d23752ac2222f21492f22a`. Consult the current checkout before implementation. This plan is a proposed implementation contract, not a claim that its APIs already exist or that any implementation/checks have been run.

The design review is paused, not complete. Durable question/answer storage, restart recovery, full coverage/baseline reconciliation, closing cross-area review checks, and richer refresh/history behavior remain for later v1 work. Do not decide or implement those systems merely to complete this demo.

## Follow-up precedence: adaptive conversation

`plan/explore-milestone-1-adaptive-interview-update.md` supersedes the remaining
fixed-questionnaire and immutable-agenda semantics in this milestone. Keep the
working-copy override and existing inline native viewers. Reviewer questions,
context and corrections now drive direct replies and attributed agenda changes;
posted questions and original decisions remain immutable. Consequential topics
expose separate reversibility and blast-radius assessments. This feedback stays
in the current milestone/change; it adds no restart persistence or source snapshots.
