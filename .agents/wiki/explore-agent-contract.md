# Explore agent contract

What the reviewer sends the agent in an Explore round (the kickoff, each wakeup, the unreviewed lines) and what the two Explore tools accept and refuse. The authoritative wording of the agent's instructions is in `crates/review-explore-runner/src/` (`interview.md`, `wakeup.md` and their siblings); this page says what the prompts and tools carry.

## The kickoff

Explore starts with a kickoff prompt containing the repository root, comparison
identity, first request ID, the change description and interview instructions. In
jj the description is the reviewed change's, quoted line by line with `> `; a Git
working tree, or a change without a description, shows `Change description: none`.
The agent inspects the code and calls `submit_question` to post the first question,
about the change's stated purpose, the design decision the rest builds on, or its largest
or riskiest unreviewed area. That first call also carries `design`, the design of the
change: `thesis`, the change in one sentence, the one a reviewer would say at a whiteboard,
then four parts: `overview` (what the change adds and where), `data_flow` (its main types
and how data flows through them), `algorithm` (the algorithm and its cost) and
`alternatives` (the alternatives the implementer rejected). Each part is an object with its
own one-sentence `thesis` and its `body` in Markdown. The tool refuses a first question
without `design`, a thesis left blank or written on several lines, a part whose body is
blank, and `design` on any later turn. A change that raises no question gets
`submit_conclusion` at once, with the design in its summary.

A round saved before designs had theses still loads, with its parts as single Markdown
texts; [CONTEXT.md](../../CONTEXT.md), "Thesis", says what stands in for each thesis.

## Earlier rounds

A Reset starts over. When the reviewer reset earlier rounds of the review, the kickoff says
how many (`Reset rounds: 2`), lists none of their decisions, and tells the agent that if its
agent session still holds those rounds, their questions, answers, decisions, agreed tasks and
question numbers are void, and that the conclusion's summary, tasks and quiz come from the new
round only. When earlier rounds stand, a task one of their decisions agreed and the code still
lacks may be listed, labelled with that decision's subject; the Explore page lists only the
round's own decisions. The review's index records how many of its first rounds were
reset when a round starts after a Reset.

When the review has earlier rounds that were not reset (a round started while the latest
was unreadable, for instance), the kickoff then lists the questions the reviewer
decided in them, oldest first, so a fresh reader such as a Challenger does not ask
them again. An answer decided its question when the agent interpreted it as `accepted`
or `needs_follow_up`, or, when the round ended before the agent's turn after it, when it
chose an option with one of these outcomes; that entry gives the option's outcome and
says it was never interpreted. Each entry gives the answer ID with the question ID and
version, the question's text, the chosen option's ID and text, the reviewer's comment
when there is one, the outcome the agent recorded and its follow-ups. The texts are
quoted line by line with `> `, as data rather than instructions:

```text
Earlier decisions of this review, oldest first (text quoted):

Decided answer: answer-id (question policy version 1)
> Keep resolved conversations resolved?
Choice: keep
> Keep them resolved
Comment:
> Include the existing caller.
Outcome: needs_follow_up
Follow-up:
> Cover the existing caller in the change.
```

Answers the agent took up without interpreting them as a decision (context, questions
to the agent), cancelled answers and replies to a conclusion are left out, and so are
rounds the reviewer cannot read, saved by an earlier version. The answer IDs are those
rounds': no interpretation or agenda change of the new round can name them, and the agent
names such a decision by its subject, never by a question number. The first
round of a review has no such section, and later turns of a round do not repeat it.

## Question numbers and schemas

The reviewer's screens number a round's questions Q1, Q2 and so on, by the steps of the
round rail, and the agent names a question by that number in every text the reviewer reads:
`submit_question` returns it as `shown_as` (`"Q3"`), and each wakeup gives the number of the
question its answer answers. Question IDs name the subject, never a number.

The tools advertise their full input schemas, including nested questions, evidence,
assessments, agenda changes and interpretations. The kickoff explains the review behavior
without duplicating schema examples.

## A round with a Challenger

A round started with a Challenger adds one section to the kickoff and a short reminder to
each wakeup. It is a script for the agent and for the subagent it starts: who proposes the
turn's question, how they exchange facts and positions, and how the single question is
written. The protocol does not change: the agent alone calls the tools, and the Challenger
only returns text to it. The agent writes the `design` and the `quiz`, as in a round without
a Challenger, and the Challenger may propose corrections to them. An agent that can continue
a subagent keeps the same Challenger for the round; one that cannot has it keep a handoff
file in the system temporary directory.

In such a round, `submit_question` and `submit_conclusion` may carry `challenger_proposals`:
what became, on that turn, of each question the Challenger proposed. Each entry has a short
`title`, the same on every turn that reports the proposal, and a `result`: `asked` (it is the
turn's question), `merged` (it was about the same decision as the agent's own question, and
the two became the turn's question), `retired` (a fact settles it) or `kept` (it waits for a
later turn). A `retired` entry also gives `reason`, plain text with the fact that settles it
and the lines that show it:
`{"title": "Retry limit", "result": "retired", "reason": "src/retry.rs new 12-14 already caps
retries at three"}`. The tool refuses an entry without a title, a `retired` entry without a
reason, and any entry in a round without a Challenger. The reviewer saves the entries with
the turn, in the round's record, and the [statistics command](explore-statistics.md) counts
them. Rounds saved before this field existed load without proposals.

## The wakeup

After a contribution of the reviewer, the shared prompt delivery sends a plain-text wakeup. It
opens with the rules for a later turn (interpreting the answer, marking lines, agenda
changes and concluding), followed, as in the kickoff, by the quiz rules and the
not-relevant rules, so those rules travel with each answer, then gives:

```text
Explore review access: temporary-access
Explore round: round-id
Explore request: turn-id
Review unit: unit
Checkpoint: commit

Answer ID: answer-id
Question: policy (version 1)
Shown as: Q1
Selected option ID: keep
Selected outcome: accepted

Selected option:
Keep resolved conversations resolved

Comment:
Include the existing caller.
```

The selected option carries its full exact text, stable ID and outcome; the comment
is preserved exactly and omitted when empty. The agent matches question ID/version
to its original question in the same agent session. The checkpoint appears once.
The reviewer keeps the full question and answer for history and validation; its
evidence, assessments, rationale, other choices and recommendation are not resent.

Conditional details appear only when relevant: Previous response error contains the
previous attempt's failure. A `Cancelled answer: <answer ID>` line, before Answer ID,
names each answer the reviewer cancelled since the previous request; the agent
disregards that answer and its own turn after it. The reviewer has already given
back that turn's review marks, and the following answer replaces the cancelled one.
An unselected option is omitted. Replies to a conclusion use Reply to conclusion
with the original conclusion turn ID instead of a question ID/version. The agent
posts the next turn with `submit_question` directly; no repository catalog or history
dump is sent, and there is no tool to fetch an answer or a round.

## Sources and citations

The agent inspects source directly from disk and uses Git/jj for diffs and historical text.
Git's `checkpoint.review_unit` identifies the base tree; jj's `checkpoint.checkpoint`
identifies the reviewed commit. Untracked files that Git diff omits are part of the change; jj merge
bases use the merged parent tree. Evidence and topic associations are cited directly as
`{path, side, lines}`: paths are repository-relative UTF-8 strings or raw byte arrays,
side is `old` or `new`, and lines are inclusive and one-based (null for file-level
references). No source registration is needed. Evidence lists the lines the question
is about, most decisive first, and explains what each establishes and how it could
change the answer. Citations never mark lines reviewed.

## Unreviewed lines and review marks

Every prompt gives the **Unreviewed lines**, the changed lines no review mark covers,
whoever marked the rest (the reviewer in Files, Jev, or the agent after an answer), as
files: an `Unreviewed diffs:` line names a directory holding one diff per changed
path, at that same path, each row prefixed with its `old` (base) and `new` (current)
line number, which are the numbers citations and marks use. The prompt itself lists no
files or lines. When a path is a file for one changed file and a directory for another,
one of the two diffs goes to the top of the directory as
`__herdr_reviewer_displaced_1__`, a `__herdr_reviewer_index__` file there lists it, and
the prompt adds a `Displaced diffs:` line naming that index. Each prompt has a new directory under the system temporary directory, readable
only by the reviewer's user; the reviewer removes it when it prepares the next prompt
or closes. A prompt whose diffs cannot be written, including when the repository cannot
be read, is not sent. After an answer of the reviewer,
the next
`submit_question` or `submit_conclusion` carries `reviewed`, the changed lines the
answer settled, and `reopened`, reviewed lines it made matter again, each as
`{path, side, lines}` (null lines for a whole file). The kickoff turn follows no
answer and cannot use them. Every turn, the kickoff included, may carry `not_relevant`:
changed lines the agent read that hold no decision for the reviewer, marked reviewed
without a question of their own. Each entry names its lines in the same form and adds
`reason`, one of `removed_code` (removed code whose removal is what the change is for),
`tested_mechanics` (mechanics that a test covers) and `follows_code` (tests, docs and
manifests that follow the code). A `tested_mechanics` entry also names the covering test
in `test`, as `{path, lines}` at the checkpoint; another entry may name one too:
`{"path": "src/parse.rs", "side": "new", "lines": {"first_line": 10, "last_line": 24},
"reason": "tested_mechanics", "test": {"path": "tests/parse.rs", "lines": {"first_line": 5,
"last_line": 30}}}`. The tool refuses a turn with an entry that has no reason, a
`tested_mechanics` entry without a test, or a test whose lines do not exist at the
checkpoint, and the error names the entry. The reviewer lists each reason and test beside
the lines it marked. Rounds saved before reasons existed keep their marks without one.
The reviewer applies a question turn's marks when the reviewer answers that question, so
review progress moves on the reviewer's action, and a conclusion's marks when it is accepted.
Marks apply only while the code is still the round's checkpoint, and what changed is
recorded for the reviewer to see.

## What a question carries

Explore displays Markdown `#` sections for Context (`rationale`, with `visual` appended),
Door and Blast radius (`assessments`), and Notes (the selected evidence's `notes`).
Supply bodies without those headings,
starting with a short summary paragraph and adding detail only when useful. Assessment
`details` may be omitted or empty; the summary must still give the decisive reason, backed
by valid evidence or explicit unknowns. All supplied reasoning and unknowns are visible
without an expansion button. Explain unfamiliar implementation concepts in Context, adapting
to the reviewer's demonstrated knowledge, while keeping questions and choices in plain language.
A choice's `text` never says that the choice is recommended, preferred or the default: the
recommendation goes only in its `recommendation` field, which the Explore page hides until the
reviewer's first pick on a question whose Door is not two-way. The tool refuses a question with
a choice whose text ends with such a mark, `(Recommended)` or `(Recommended: the safest)`, and
names the choice; `(recommended by RFC 9110)` names someone else's advice and passes. Choices, their recommendations and follow-ups are plain
text; on the Explore page their Markdown code spans show as code.

## Submitting a turn

The agent sends each complete structured turn with `submit_question`, with the supplied
review value and the result in `update`. `submit_question` requires a next question
and cannot carry a conclusion.
Concept exploration drives the interview: reviewed lines do not exhaust its useful
questions. When no useful inquiry remains, the agent checks the unreviewed lines for
missed concepts, asks further questions only when that reveals one, and then
concludes. With a reviewer-process `TYPESAFE_API_KEY`, Jev marks what it judges
insignificant when a round starts; the prompts do not mention it.
The durable `instance` is distinct from renewable `review` access. Access is never
saved with the round. Reopening rotates it; the next explicit reviewer action supplies
current access through the existing wakeup. Each call checks the pinned native
agent session, then validates against the latest stored round under its lock, atomically
saves the update and deduplication record, publishes it to the UI, and waits for UI
application before acknowledging it. Validation errors leave the request open for repair;
transport retries must reuse the identical semantic payload (with current `review` access after reconnection). A response saved before a lost acknowledgement is restored locally; it is not regenerated. A question's result also gives
`shown_as`, its number on the reviewer's screens. Accepted retries return
`accepted: true, applied: false`. Cancelled, obsolete or changed accepted payloads
are rejected. A submission is bounded to 1 MiB; a larger one is reported, not truncated.
Explore never writes ordinary thread replies or uses response files.

## The conclusion

`submit_conclusion` carries the separate conclusion screen. Its top-level arguments are:

```json
{
  "review": "temporary-access",
  "instance": "round-id",
  "request": "turn-id",
  "checkpoint": {"review_unit": "unit", "checkpoint": "commit"},
  "interpretation": null,
  "summary": "Review outcome and remaining uncertainty.",
  "to_be_implemented": "1. First agreed task.\n2. Second agreed task.",
  "future_work": "Optional or later work outside this implementation scope.",
  "quiz": [],
  "quiz_empty_reason": "The change only rewords the summary toast."
}
```

The three sections are separate strings. Use an empty string for no implementation
or future work. The final answer still needs its attributed interpretation when it
records a decision; the same exact-answer and retry rules apply. There are no
question, evidence, reply, topic or agenda fields in a conclusion submission.
`quiz` holds at most three questions at whiteboard level, each `{question, answers,
correct, why, proof, level}`: two to four `answers`, the zero-based index of the
`correct` one, the sentence that says `why`, the `proof` citations of the lines that
establish it, and `level`, the agent's own reason the item is at whiteboard level, which
the round keeps and the page does not show. An empty quiz needs `quiz_empty_reason`; a
quiz with items leaves it null. The tool refuses an item with no correct answer or no
proof lines. The one prompt asks for the quiz wherever the reviewer follows the round:
the Explore page asks it and grades each pick itself; the pane does not ask it.
Summary and future work are displayed separately. Only `to_be_implemented` seeds
the editable task box. Submitting a conclusion does not start implementation.
A conclusion saves its outcome before acknowledging success; it can carry `reviewed`
and `reopened` for the final answer like any turn after an answer, and changes no
other review marks. Lines no answer settled stay unreviewed for the reviewer.
The reviewer's **Implement** action sends the edited box contents through the shared
reviewer-to-agent delivery queue, authorizing those tasks and their validation.
It waits for the pinned agent session, supports cancelling queued delivery,
and reports delivery failures without discarding edits. Authorization saves the exact
edited scope and logical delivery ID before queuing. The shared dispatcher saves an
attempt marker before the external call and records the authoritative outcome before
reporting success. Reopening never replays pending work. An unfinished attempt is
shown as delivery unknown, not as a cancelled or definitely unsent request.
Delivery confirmation
means the request was sent, not that implementation has finished.
