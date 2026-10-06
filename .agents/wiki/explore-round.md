# Explore round: behaviour

What an Explore round does for the reviewer, from Start to Reset: how it starts and where it shows, what is saved, how answers move review marks, what Cancel answer, the conclusion, Implement and Reset do. The terms are defined in [CONTEXT.md](../../CONTEXT.md).

Related pages: what the agent is sent and must send is in [Explore agent contract](explore-agent-contract.md); how the code does it in [Explore protocol: internals](explore-protocol.md) and [Explore recovery](explore-recovery.md); the page's own rules in [Explore page: behaviour](explore-page-behaviour.md); the settings in [Explore settings](explore-settings.md). Explore is experimental.

## Starting a round

- Start and Start with Challenger start a round and open its Explore page in the default
  browser of the machine that runs the reviewer. The round is then a round on the page: the
  pane's Explore tab shows no question, answer or conclusion, only that the round runs on
  the page, where it stands in one line (starting, the agent is working, a question waits,
  concluded, or interrupted and why), Open the Explore page, Reset, and the page's address
  and QR code. A round started on the page shows the same way.
- Continue in the pane shows the interview in the pane instead. It is the same round, and
  the reviewer remembers the choice when it is reopened.
- When the browser cannot open the page (no browser, no display), the round still starts,
  and the pane says why and gives the page's address.
- Start in the pane, with or without a Challenger, starts a round in the pane and opens
  nothing. With the setting "Open the page on Start" off, Start does the same.
- A round covers the complete change from the base to the working copy, including files
  already reviewed and files the reviewer filtered out.
- When Jev is enabled, a start first marks what [Jev](jev.md) dismisses; the agent sees
  only what is left.
- The agent is the selected implementation agent, in its existing agent session. It asks at
  most one next question per turn. Prompts go through Herdr at once, also while the agent
  works, by the same delivery as thread comments ([Review threads](threads.md)). Each
  delivery uses the selected agent, and an attempt under way stays bound to that selection
  until it resolves.

## When nothing is left to review

- When every changed line is marked as reviewed, a round has nothing to ask: the start
  buttons stay on the start screen, inactive, with a line that says so, and their keys do
  nothing. Unmarking a line or a file makes them active again without reopening the
  reviewer.
- A start that finds nothing left once it began, because Jev or another reviewer marked the
  rest meanwhile, sends no kickoff and says why.
- A round that already runs goes on when its last line is marked.

## A round with a Challenger

The Challenger reads the same prompts and the same diffs as the agent. Each turn, the two
each propose a question. They take turns having
theirs asked, the Challenger first; the other's waits and is judged again after the answer.
The one that did not propose the question gives its facts and position on it, and the
question the reviewer reads is one text written from both. The reviewer still answers one
question per turn, and nothing else changes except that turns take longer. The round ends
when neither has a question left. The choice holds for the round.

## What the agent writes

- The agent writes every text of the round in the writing style the round started with.
- Whatever the style, when the design or a question presents a behaviour change, the agent
  says what changes, then why (the problem, what goes wrong without the change, and where
  that reason is stated, or that it infers it), and only then how.
- The agent keeps the round's context in its own agent session. A wakeup carries the answer
  and its identities, not the history, so a newly selected agent session may need the
  reviewer to supply what it lacks. There is no history dump, repository catalog, mailbox
  or file fallback.

## The code stays unchanged during a round

- Explore reads working-copy files directly and assumes that they stay unchanged during a
  round, also across a reopen. It takes no snapshot of the repository and does not pause
  when a source file changes.
- The saved decisions describe that investigation. They do not establish that later edits
  were reviewed: the reviewer resets and starts a new round when the code has changed.
- Review marks from the round apply only while the code is still the checkpoint the round
  started from; what changed is recorded for the reviewer to see.
- Supporting files are read as needed, so a large unchanged asset imposes no
  repository-wide limit.

## What is saved

- Progress is saved automatically. Opening the same checkout and review restores its
  latest round: the exact questions, answers, agenda and conclusions, the review marks
  each answer led to, the separate task and reply drafts, the selected choice and the
  reading position. A round saved by a version from before answers could mark lines is not
  restored.
- Reopening sends no prompt and never starts an implementation. The next explicit action
  prompts the selected agent. When that agent is unavailable, the history and the edits
  stay until an agent can be selected and the action retried. A new native agent session
  alone does not require a new round.
- Accepted responses, posted answers and the authorization of an implementation are saved
  before they are acknowledged or delivered.
- Editor changes are saved in the background and flushed on a normal close; an abrupt
  death of the process can lose keystrokes still waiting for a save.
- A write error stops the Explore changes that would be unsafe. Files and Threads stay
  usable.
- Explore supports one agent and one reviewer per repository, with one saved editor and
  reading state per round.
- A corrupt, oversized or unsupported record is cleared with an error toast when
  possible, and readable history is kept; when clearing fails, Explore reports the
  storage error. [Explore recovery](explore-recovery.md) gives the bounds of a record.

## A question

- A question offers two to five alternatives and None of the above. In the pane the first
  is selected by default.
- The text field adds optional details to the selected choice, and changing the choice
  keeps them. One Send submits both. None of the above keeps the inquiry open, with or
  without text.
- On a follow-up turn, the agent's reply shows above the question. The question is followed
  by its choices and the text field, then by Context, Door and Blast radius, then Notes
  and the evidence. Each section opens with a short summary paragraph.
- Door assesses whether the effects can be undone, including the conditions of a rollback
  or a rebuild. Blast radius describes the plausible harm, how it spreads and its bounds.
  Both are the agent's judgements, backed by evidence; neither is a risk score or a
  guarantee.
- The recap shows the agent's interpretation of the answer. Free text may ask why, request
  a caller, challenge an assumption or add context; the agent replies directly, and a
  factual question or added context is not agreement. A conditional decision keeps its
  required changes as follow-ups. The interpretation of free text is something to check
  and to amend with another answer, not a semantic guarantee.
- The pane shows one question at a time, with a history of questions and conclusions in
  posting order. Its evidence, answers and recap scroll together, and the recap moves
  below the answer once the answer is sent. Each question keeps its own unfinished text, selected choice and evidence
  state.
- A narrow terminal reflows the question and the history controls, and leaves the
  sidebar of Files as it is.
- The pane can expand the provisional map beside a question: the agenda's states, its
  prerequisites, the entries not yet mapped and the limits of the scan.
- Each accepted new question opens by itself, also after input while waiting. While the
  reviewer is in Files or Threads, Explore selects it for their return without switching
  panes.

## When the round ends

- The exploration of concepts decides when the discussion ends. The agent does not keep
  asking only to mark more lines.
- Once the agent has exhausted its agenda, it checks the unreviewed lines for missed
  questions, then concludes. Lines no answer settled stay unreviewed.
- The agenda is provisional. Context can add, refine, reorder, retire or supersede pending
  inquiries. A retirement keeps its reason and the original wording and does not mean
  acceptance. A reconsideration flags an earlier conclusion without changing the original
  decision; its conditions stay outstanding. New questions imply no fixed total.

## Evidence in the pane

- Evidence lists the question's citations, most decisive first, then those its
  assessments, the agent's reply and its agenda changes cite. A citation of the same range
  shows once.
- Every cited range of the shown file is outlined in yellow. The outline means "relevant
  to this question", not accepted, reviewed or high risk.
- The first evidence window fits the cited rows, wrapped, with context, up to about half
  the content height; larger sources scroll. A manual height, and each viewer's position,
  search, selection and comment draft, stay independent. Overlapping ranges share an
  outline and fit together; distant ranges stay separate.
- An old-side citation opens at its old coordinates. A range outside the displayed hunks
  opens the full base text, never the working-copy text in its place.
- A source that is not text, or is unavailable, shows a short limitation.

## Review marks from the answers

- After each answer, the agent marks the changed lines the answer settled as reviewed, and
  can reopen reviewed lines the answer made matter again, whoever marked them. These are
  ordinary review marks: Files, the diff and the header's progress show them, and the
  reviewer can reopen or mark hunks as usual.
- On any turn, the first included, the agent can mark lines it read as not relevant, each
  with its reason. A mark for mechanics that tests cover names the test, its file and its
  lines, so the reviewer can check it:
  `src/parse.rs new 10-24 (not relevant: mechanics covered by tests, see tests/parse.rs 5-30)`.
- Review progress moves when the reviewer acts. The marks that come with a question wait
  until the reviewer answers it: until then the question says what it will mark. So the
  lines an answer settled are marked when the reviewer answers the next question, or at
  once when the agent concludes instead.
- A conclusion can mark the lines the final answer settled. Citations and topic
  associations mark nothing. Explore resolves no review thread.
- Every prompt points the agent at the unreviewed lines, the reviewer's own marks from
  Files included. When their diffs cannot be written, or the repository cannot be read,
  the prompt is not sent, a toast says why, and Retry tries again.

## Waiting, Stop waiting and Retry

- While a round prepares or the agent works, the pane shows the actual pending status and
  Stop waiting. A delivery error and Retry show beside the turn they affect.
- When the agent does not start on a prompt within a few seconds, the turn says so: its
  text may still wait in the agent's prompt box. Retry sends the same turn again. An
  implementation request the agent did not start on offers Retry too.
- An invalid response leaves the last usable question and answer available. A validation
  error lets the agent repair the same pending turn; an identical retry of an accepted
  result is acknowledged without being replayed.
- Request and answer identities protect against cancelled, duplicate and unrelated
  responses. They do not establish that the source is fresh.

## Cancel answer

- Cancel answer takes back the reviewer's latest answer, also while the agent still works
  on it. The agent's turn after it is discarded, and the review marks that turn made are
  given back: lines it marked reopen, and lines it reopened return to whoever had marked
  them.
- The question comes back with the choice and the text ready to change and send again.
  Repeating it takes back earlier answers one at a time.
- The next prompt tells the agent which answers were cancelled.
- An answer cannot be cancelled once Implement was sent.
- When the code changed since the round started, lines the cancelled turn reopened stay
  open.

## The conclusion and Implement

- The conclusion separates Summary, an editable "To be implemented" box, and Future work.
  Earlier conclusions keep their own edits and replies; only the current one offers
  Implement.
- Implement authorizes the agent to implement exactly the list in the box. Summary and
  Future work are not added to the request.
- A queued delivery can be cancelled; an error keeps the edits.
- After a reopen, a request that was never attempted stays paused: the reviewer can send
  it as it was authorized, or make a new request from the box as it is now. A delivered
  request is not sent again by itself.
- "Delivery outcome unknown" means that a crash or a transport failure may have interrupted
  the confirmation: the reviewer checks the agent's session before sending a new
  request on purpose.
- A sent status confirms the delivery, not that the implementation is done.
- Reply continues the interview about the conclusion and authorizes no code change.

## Reset

- Reset closes the round and returns to the start screen. It needs Confirm reset within
  five seconds; any other action cancels it.
- The closed round stays saved but is no longer shown, and its agent can no longer post to
  it. Reopening the reviewer after a Reset shows the start screen.
- The next round starts over: its kickoff lists none of the reset rounds' decisions and
  tells the agent that their questions, answers, decisions, tasks and question numbers are
  void, even when its agent session still holds them.
- The review marks that the reset rounds applied stay.
