# Milestone 1 update — an inline review conversation

Apply this UI correction to the milestone 1 implementation already in progress. **Do not restart the feature or implement a later milestone.** This instruction replaces the side-by-side layout in §5 of the milestone 1 plan and updates its UI acceptance criteria. Other milestone 1 behavior and safety boundaries remain in force.

## Follow-up: review the working copy

The user subsequently removed Explore snapshotting for this milestone. Assume
code stays unchanged during review. Read current sources from the working copy,
including supporting callers/tests, and remove repository capture limits and
stale/unknown-source decision pauses. Keep the complete change manifest, base-side
comparison text, attributed answers, cancellation and response identity checks.
This supersedes the frozen-source and freshness requirements below and in the
original milestone plan; ordinary Files baselines and comment semantics still apply.

The next UI feedback removes the persistent yellow working-copy/unsaved-progress
warnings and shortcut documentation from the flow header. Separate the flow's
question, evidence, answer, status and action blocks with blank lines. This
overrides the persistent notice requirement below; contextual controls and help
remain available.

Further feedback adds answer selection with Up/Down or j/k, with the first
alternative selected by default and Enter confirming it. Free text remains
reachable through the same selection. A late first report of the native agent
session must preserve an existing interview; once known, an actual session
replacement still requires a new pass.

## Replace the layout, not the review engine

The current screen puts the interview in a narrow file-list sidebar, gives an empty diff most of the screen, separates the answer from its question, and permanently displays too many commands.

Replace that arrangement with **one vertically scrollable interview flow, containing fully functional, resizable diff windows between conversational turns**. Do not merely widen the sidebar or implement a 40/60 split. Explore should use the available content width independently of Files' sidebar setting; leave Files and Threads layouts unchanged.

Proposed shape; labels and source below are illustrative:

```text
[F]iles  [T]hreads  Explore                  Frozen · Not saved after closing
---------------------------------------------------------------------------
Question 1 · Should a late reply reopen a resolved thread?

  reply.rs · checkpoint abc123                 [State] [Test] [Fit evidence]
  +---------------------------------------------------------------------+
  | surrounding code                                                    |
  | +-- yellow relevance outline -------------------------------------+ |
  | | actual diff rows                                                | |
  | +-----------------------------------------------------------------+ |
  | surrounding code                                                    |
  +----------------------- drag to resize ------------------------------+

You: Keep it resolved.
Recorded: keep resolved.                                  [Collapse turn]

Question 2 · ...
  [its embedded, fully functional diff window]

A  ... — recommended: ...
B  ...
Or answer in your own words:
[                                                                    ]
[Send]  [Defer]                             > Why this matters
```

The transcript accumulates in order; only the current question has an active answer composer. Answered turns retain their exact questions, answers, and evidence references. Offer collapse/expand for earlier turns, without automatically collapsing something the reviewer is reading. Do not force a separate History screen just to reread the previous answer. Place options, composer, and primary actions in normal document flow, not docked at the terminal's bottom.

## Embed the real diff view and size it around the evidence

Reuse the existing functional diff/source component, including syntax highlighting, search, selection, ordinary comments, source navigation, and the milestone's LSP behavior. **A fixed excerpt, Markdown code block, screenshot, or lookalike renderer is not a substitute.** The initial viewport is small; the underlying source remains fully browsable.

Initially open the question's primary evidence. Supporting-evidence links reveal or focus the corresponding inline viewer on demand; do not open an entire file-sized window for every reference. Keep source/checkpoint labels visible. Following a definition must not change the question's recorded evidence identity; provide a way back to primary evidence.

Default the window height to the rendered relevant range, a few surrounding context lines, and necessary borders/header. Account for wrapped rows, not just source-line counts. Cap the initial height at approximately half the available content height so a large range cannot bury the entire interview. Oversized evidence remains scrollable, with honest continuation indicators.

For distant/disjoint ranges, initially fit the primary range and make the others navigable. Do not create a huge window spanning unrelated code between the first and last ranges. Yellow outlines remain separate and mean relevance only. Non-text or unavailable evidence gets an explicit compact state, not a blank viewer or invented line box.

Allow both mouse dragging of the bottom edge and a keyboard resize action discoverable in help. **Fit evidence** restores automatic sizing. Preserve manually chosen height per evidence block for this open exploration; background updates must not reset it. Clamp safely on terminal resize. Recalculate fit-mode heights when wrapping changes. Resizing must not lose the code position, selection, comment text, or answer draft.

## Make scrolling, focus, and waiting predictable

Distinguish scrolling the conversation from scrolling a diff. Mouse wheel input over a diff scrolls that viewer; outside it, it scrolls the conversation. Keyboard input goes to the visibly focused editor/view. Consume each event once: do not scroll both layers or turn typed answer characters into navigation commands. Provide a clear keyboard route between the conversation, evidence, and composer using existing conventions.

On normal advancement, reveal the next question and automatically position its primary evidence. Preserve the existing no-focus-stealing rule: if the reviewer starts typing, investigating code, or reading history while waiting, retain their position and show that the next question is ready. Pending-map updates alone must not move the viewport. Expanding earlier evidence must restore that block's state rather than replace the active question's source or answer.

Before the first question exists, show a compact preparation state with Cancel and continued access to Files. **No empty diff frame, inactive answer editor, or command wall.** After submitting an answer, retain the question, evidence, and submitted answer; put the waiting status underneath. Actual delivery delays, errors, cancellation, and Retry belong beside the affected turn. Do not invent progress stages.

Keep Send/Defer near the current answer. Put map, details, correction, retry, new-pass, and other secondary actions behind contextual controls or help; only expose them when relevant. Keep the unsaved-progress notice compact but explicit. Stale/unknown source warnings must stay prominent and retain their existing decision restrictions.

## Implementation boundaries

Inspect the current component ownership and input routing before changing them. Reuse shared rendering/navigation services; give embedded viewers distinct source/focus/view state so interacting with one cannot corrupt another. Mount expensive viewers as needed rather than starting a separate language server or duplicating the diff engine per turn.

Retain the existing agent-turn protocol, attributed answers, frozen-source checks, cancellation/late-response protections, file-level review marking, and ordinary comment semantics. No second Accept click. No automatic review marks from viewing code or answering questions. No persistence, classifier, historical LSP workspace, or backend redesign added for this UI correction. Apply the original snapshot/LSP provenance rules to every embedded viewer.

## Demonstrate this correction before adding features

Add deterministic layout/input regression tests using the existing isolated test infrastructure, then demonstrate:

1. Initial waiting, ready, submitted/waiting, error/retry, and stale states: no empty dominant pane or irrelevant controls.
2. Two real question/answer turns in one flow, with inspectable earlier evidence and a nearby composer for the current question.
3. A small yellow range producing a fitted viewer; a large/wrapped range remaining bounded; mouse and keyboard resizing plus Fit evidence.
4. Search, LSP, selection, and an ordinary code comment inside an embedded viewer, without losing the interview draft or mixing source versions. Resizing a comment-bearing viewer must remain usable.
5. Supporting evidence, history expansion, narrow-terminal reflow, and response arrival while reading history: correct focus, independent view state, and no unexpected jumps. Files/Threads and file review marks remain unchanged.

Use fixtures with both short and long questions/code. Exercise all normal code-view capabilities rather than accepting a visually convincing mock. Run the project's required checks and feedback-fix workflow; do not use the live user workspace as a test fixture. Update only the affected UI/help documentation. Present what actually works, note limitations, and stop for feedback within milestone 1.

## Follow-up precedence: adaptive conversation

`plan/explore-milestone-1-adaptive-interview-update.md` supersedes the remaining
fixed-questionnaire and immutable-agenda semantics in this milestone. Keep the
working-copy override and existing inline native viewers. Reviewer questions,
context and corrections now drive direct replies and attributed agenda changes;
posted questions and original decisions remain immutable. Consequential topics
expose separate reversibility and blast-radius assessments. This feedback stays
in the current milestone/change; it adds no restart persistence or source snapshots.
