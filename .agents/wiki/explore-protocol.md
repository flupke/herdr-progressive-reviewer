# Explore protocol: internals

How the reviewer handles an Explore round's turns: accepting a submission, delivering a prompt, the design, the writing style, later wakeups, the agenda, code locations, and what the pane renders.

The wording of the agent's instructions is in `crates/review-explore-runner/src/`;
`interview.md` is the authoritative contract. [Explore agent contract](explore-agent-contract.md)
says what the prompts and the tools carry. This page says how the reviewer handles them.

## A submission

- One `InterviewUpdate` binds a direct reply, an optional interpretation of the decision,
  agenda operations and one next question to the exact outstanding request.
  `submit_question` carries it.
- `submit_conclusion` carries a separate `ConclusionSubmission`: the summary, the editable
  implementation tasks, the future work, the quiz and the interpretation of the final
  answer.
- A submission needs the exact request ID, the access value and the pinned agent session.
- `Exploration` validates every citation and the agenda before it applies anything.
- Both tools go through the reviewer's server to a locked store transaction, then to the
  UI. Success acknowledges the validation, the durable commit and the application.
- An invalid update stays pending and returns its error to the agent for repair. A
  cancelled or obsolete request cannot apply. An exact duplicate is acknowledged without
  being replayed, and a different payload for an accepted request is rejected.
- The tools advertise complete schemas derived from the shared submission types. The
  kickoff carries behavior instructions without schema examples.
- The protocol validates the integrity of references, not the agent's reasoning. It cannot
  establish mechanically whether free text is agreement, or whether an assessment is true:
  the visible replies and recaps, the evidence, follow-up answers and the reviewer's
  inspection remain necessary. An interpretation is optional for a contribution that only
  informs.

## Prompt delivery

- Thread comments and Explore turns go directly through Herdr's `agent.prompt`, which waits
  until Herdr sees the agent working or blocked. An agent that works already counts at
  once. One that shows neither within Herdr's 5 seconds gets `agent_prompt_stalled`, saved
  as `NotStarted`, which Retry may send again with the same request.
- A courier thread sends the prompts in order, so the worker keeps serving while Herdr
  waits.
- Delivery does not parse terminal output, and does not wait for an idle agent, an
  unfocused pane or an empty composer.
- Review access binds to the native session when there is one, and to the foreground
  process group otherwise.
- A cancellation removes the requests not yet sent, and a replaced agent session rejects
  them. A delivery failure keeps the literal input for Retry.
- Finding the agent session, and handling its replacement, do not depend on whether the
  repository changed.
- An answer reaches the agent through this delivery only; no file carries it.

## The kickoff and the design

- The kickoff carries the scope, the change description (jj only; Git has none) and the
  identity of the first turn. The agent submits its first question directly.
- Only the first turn may carry `design`, the design explanation, and its question must
  carry it.
- The design opens with a thesis for the change, and each of its four parts opens with
  its own. Code that shows the design reads them through `Design::thesis` and
  `Design::parts`, which give a thesis in every case.
- For a round saved before theses existed, the overview's first paragraph stands in for
  the change's thesis, and each part's first paragraph (the overview's next one) for its
  own, on one line and left out of the part's text. Only a paragraph of its own counts,
  not one inside a list or a quote. A part without one shows its first line of text and
  keeps all of it, and an overview of one paragraph shares the change's thesis.
- A message to the page carries the theses and the parts, never `Design` itself, which
  saves such a round's parts as the plain strings they were.
- The pane shows the design before the first question, and the page shows it as a screen
  of its own, the first of the round ([Explore page: behaviour](explore-page-behaviour.md)).

## Writing style

- A round's writing style (`WritingStyle` in
  [`crates/review-explore-round-settings`](../../crates/review-explore-round-settings))
  comes from the reviewer's settings for the next round when the round starts
  (`ExploreRoundSettings`, saved by `ReviewStore` in `settings.json`, changed in the pane
  with `W` through `SettingsAction::SaveExploreWritingStyle`).
- `Exploration` and every `TurnRequest` keep the style, so it survives a restore.
- In Simplified Technical English, the kickoff ends with the style's rules (`writing.md`)
  and every later turn with a short reminder (`writing_wakeup.md`), since the agent of a
  long round may have compacted the kickoff. The plain style adds nothing.

## Later wakeups

- The runner formats a later wakeup as the later-turn rules (`wakeup.md`: interpretation,
  review marks, agenda changes, concluding), then labelled plain text: the turn's identity,
  the checkpoint once, the answer ID, the question's ID and version, the selected
  option's full text, ID and outcome, and any exact comment, after the Unreviewed diffs
  line.
- The context of a conclusion, and a previous error, add details only when relevant.
- The full question and answer records stay internal, and preparing a wakeup does not
  change that history. There is no tool to fetch an input, and the runner keeps no input
  of its own: the reviewer keeps the immutable history, and the agent keeps its context in
  its agent session.
- A contribution to a stopping point that has no question carries "Reply to conclusion",
  which names its closing turn. That context cannot be interpreted as a policy decision.

## The agenda

- The lifecycle of an agenda topic (retire, supersede, reconsider) is separate from the
  status of a decision.
- A topic holds its pending wording, its order and the IDs of its prerequisite topics. The
  posted versions and the agent's operations are kept in the round's history.
- A reconsideration names the original decision, and a later attributed decision resolves
  the flag.

## Code locations and sources

- `CodeLocation` carries a repository-relative path, the old or the new side, and an
  optional inclusive line range. A path is a UTF-8 string, or a byte array for a name
  that is not UTF-8. Topic associations use the same locations.
- There are no source IDs and no call to register a source.
- The internal comparison keeps the context of the changed files for the native viewers,
  without listing every file of the repository. An unchanged source is resolved on
  demand: a historical file from the original base, a new-side file read directly.
- The agent inspects files on disk and uses Git or jj for diffs and historical versions.
  In Git, the review unit identifies the base tree; in jj, the checkpoint identifies the
  reviewed commit.
- A destination a language server leads to is read only inside the repository root.
- Historical evidence outside the diff hunks uses a native view of the full base text,
  and cannot send old coordinates to the live language server.

## Questions and citations

- A question's evidence is one ordered list of citations, most decisive first. The
  citations of the reply, of agenda reasons and of the assessments come after it.
- Every citation requires `notes` that explain its relevance. A citation never marks
  lines reviewed.
- Citations are deduplicated by path, side and range, and keep stable viewer identities.
- Every next question requires two to five distinct alternatives. `Question::choices`
  adds the built-in None of the above, with a stable ID and an open outcome, without
  rewriting the posted question. Its ID and label are reserved, so an alternative of the
  agent cannot duplicate it.
- The selected choice and the edited text are one answer: Send records both.

## What the pane renders

- Explore renders only the selected question, with a pinned bar to navigate the history.
  The question shows directly above the answer form.
- A newly accepted question selects the newest page, also after ordinary input. Applying
  a duplicate does not navigate.
- Files and Threads keep their active pane while Explore prepares its next page.
- The question's identity also selects the composer draft and the evidence window.
  Navigating the history saves the full editor state before it restores the destination's
  draft.
- The size and the first position of an evidence window use the same wrapped range,
  yellow borders included. Fitting again centers the range, not its first line.
- Conclusion pages are kept by request identity, in posting order among the questions.
  Each keeps its task editor, its reply draft and its delivery state. Only the current
  conclusion can start an implementation.
- Implement sends only the task list as the reviewer edited it, through the shared prompt
  queue. The result event confirms the delivery, not that the implementation is done.
