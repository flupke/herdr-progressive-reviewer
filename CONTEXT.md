# Progressive review

A review tracks the examination of changing code and the discussions about it.
Review progress and those discussions remain meaningful across code changes.

## Language

**Review**:
One logical unit of code being examined. Its identity continues across changes to
that code.
_Avoid_: review session

**Review checkpoint**:
A particular version of the code within a review.
_Avoid_: review

**Review thread**:
A discussion about a code selection within one review, or the round conversation of
one Explore round. It remains part of the review when the selected code changes or
disappears.
_Avoid_: agent thread, conversation without qualification

**Thread message**:
A posted contribution to a review thread, written by the reviewer or an agent.
Messages form the thread's conversation in posting order.

**Review comment**:
A thread message containing the reviewer's question or feedback.
_Avoid_: agent prompt

**Agent reply**:
A thread message from an agent responding to the conversation. It may address
several review comments.

**Draft**:
Review-comment text that the author is still composing. It is not part of the
shared review thread.
_Avoid_: unposted text, queued comment, saved-but-unsent comment

**Posting**:
The reviewer's publication of a comment to a review thread.
_Avoid_: saving a draft

**Agent session**:
One ongoing conversation with a coding agent, which may address several review
threads.
_Avoid_: review thread, review session

**Review mark**:
The record that some changed lines of a file are reviewed, and who marked them:
the reviewer, Jev, or the Explore agent, after one of the reviewer's answers or
for lines it found not relevant.
_Avoid_: coverage, credit

**Reviewed version**:
A file as the reviewer has read it: the base with the changed lines accepted as
reviewed. A whole-file mark makes it the file at that review checkpoint.
_Avoid_: baseline (a baseline is the commit a mark was made at)

**Unreviewed lines**:
The changed lines of a review checkpoint that no review mark covers.
_Avoid_: gaps, uncovered regions, remaining coverage

**Jev**:
The classifier that marks changed lines too insignificant to need a reviewer's
attention.

**Open hunk**:
A change from the reviewed version to the current file, still waiting for
review. It is "changed since review" when it rewrites reviewed lines.

**Reviewed hunk**:
A change from the base that the reviewed version holds and the current file
still matches. The diff folds it into one row.

## Explore

**Explore round**:
One interview in which an agent questions the reviewer about a review
checkpoint, and marks or reopens changed lines according to the answers. A new
round keeps earlier rounds as history.
_Avoid_: Explore pass, Explore session

**Design explanation**:
What the first turn of an Explore round explains before its first question, so the
reviewer could explain the change at a whiteboard: a thesis for the change, then four
parts, each with its own thesis: what it adds and where, its types and data flow, its
algorithm and cost, and the alternatives the implementer rejected.
_Avoid_: map (the provisional map of the first reply, which it replaces)

**Thesis**:
One sentence of a design explanation, the one you would say at a whiteboard: one for
the whole change, and one that opens each of its four parts. The reviewer reads the
five theses first; each part's text comments on its thesis. In a round saved before
theses existed, the first paragraph of the first part stands in for the change's thesis,
and the first paragraph of each part (the next one for the first part) for its own.
_Avoid_: summary, lead paragraph

**Writing style**:
How the Explore agent writes every text the reviewer reads in a round: plain (the agent's own
style) or Simplified Technical English (the writing rules of ASD-STE100, without its controlled
dictionary). The reviewer's setting gives it to a round when the round starts, and the round
keeps it on every turn.
_Avoid_: tone, language

**Challenger**:
A subagent with fresh context that the Explore agent starts for a round, to
review the same change without the implementer's knowledge of why it was
written. The two take turns proposing the round's questions.
_Avoid_: adversary, second reviewer, critic

**Challenger proposal**:
A question the Challenger proposes for a turn, and what became of it: asked,
merged with the implementer's question, retired because a fact settles it, or
kept for a later turn. The Explore prompts call each side's question for the
turn its candidate, so a proposal is a Challenger's candidate other than
"conclude". Retiring a proposal is not retiring an agenda topic, which needs an
invalidated premise.
_Avoid_: suggestion, Challenger question (which may never be asked)

**Citation**:
A path, side and line range that a question puts before the reviewer as
evidence, most decisive first. Citing lines does not mark them reviewed.
_Avoid_: source reference, inspection, supporting reference

**Quiz**:
The few questions at whiteboard level (architecture, algorithms, data storage, the
data model) that the Explore agent submits with its conclusion, each with its correct
answer, why, and the lines that prove it. The Explore page asks them one at a time
before the conclusion and says at once whether each pick is correct; the reviewer may
skip them. The round saves the picks. A change with nothing at that level gets no quiz.
_Avoid_: test, exam, check

**Callout**:
A quote in the agent's Markdown that opens with a marker, such as `[!TIP]`, for a
conclusion, a tip, a warning or an error. It shows as a block titled with its kind.
_Avoid_: alert, admonition

**Diagram**:
A fenced `mermaid` block in the agent's Markdown, which the Explore page draws
with Mermaid. A diagram Mermaid cannot parse shows as its source, and its error
is saved with the question.
_Avoid_: chart, figure

**Status mark**:
A good, bad or warning mark at the start of a table cell in the agent's Markdown,
written as a marker such as `[!good]`.
_Avoid_: badge, emoji

**Not relevant**:
What the Explore agent calls changed lines it read that hold no decision for the
reviewer. They get a review mark without a question of their own, and a reason:
removed code the change is about, mechanics a named test covers, or tests, docs
and manifests that follow the code.
_Avoid_: skipped, ignored, insignificant (which is Jev's judgement)

**Round conversation**:
The review thread attached to one Explore round instead of a code selection, where
the reviewer asks, challenges or adds context beside the round's questions. Each of
the reviewer's messages names the question, by identity and version, or the stage
(the design, the conclusion) it was asked under, and may quote a passage of the round.
A message never answers the question, which stays open; the agent answers with a
thread reply. The round's first message starts it, and each round, including one
started after a Reset, has its own. The Explore page calls its view of it the chat.
_Avoid_: chat thread, agent chat (in the domain model), Explore turn

**Cancel answer**:
Withdrawing the reviewer's latest answer in an Explore round, which returns the
round to its state before that answer, including the review marks it led to.
_Avoid_: cancel (which stops a pending turn), correction, undo, defer

**Explore page**:
A browser page, served by the open review pane, that shows the current state of
its Explore round: no round, a round starting or failing to start, the agent
working, the agent's question with its choices, a turn the agent is no longer
working on, or the conclusion with its quiz; and, on screens of their own, the
design explanation and each earlier question. It follows the round as it changes.
The reviewer can run the whole round there, as in the pane: start it, answer,
stop waiting, retry, cancel an answer, implement it and reset it; and write in the
round conversation. Its address carries a token, and the page refuses requests without it.
The pane serves it on this machine, for the Herdr action and for Start and Start
with Challenger, which open it in the browser (Start in the pane does not), behind a
token that the next pane of the same review keeps, so that an open page reconnects
after a restart, and on the network, behind a new token for each round, for the QR code in the pane; while
no round runs, the start screen's token is the one the next round keeps, and a
Reset from the page hands that page the next one.
_Avoid_: web UI, Explore web, browser view

**Round on the page**:
An Explore round started with Start or Start with Challenger, or on the Explore
page. The pane shows only that it runs on the page, where it stands, and the
page's address, until the reviewer chooses Continue in the pane. A round started
with Start in the pane is a round in the pane, which shows its interview.
_Avoid_: web session, web round

**Round rail**:
The steps of an Explore round as the Explore page lists them: the design
explanation, each question the agent posted with its clarified versions, the
quiz and the conclusion, each done, current or later. Each done question opens its
earlier question. Its question steps are numbered Q1, Q2 and so on, and the agent
names each question of the round by that number.
_Avoid_: stepper, breadcrumb, progress bar (the meter is the lines reviewed)

**Earlier question**:
A question of the current Explore round that the reviewer already answered: a done
step of the round rail. The Explore page shows it read only, with the answer kept,
the review marks it led to, and what the agent recorded of the answer.
_Avoid_: previous question, history, earlier round (a round that a newer round of
the review replaced)

**Meter**:
The line under the Explore page's masthead that shows the share of the changed lines
that review marks cover, split by who marked them, with the lines the question that
waits marks once answered and the lines left; its window gives the numbers for the
change and for each file.
_Avoid_: coverage, progress bar

**First pick**:
The choice the reviewer picks on the Explore page before it shows the agent's
recommendation, on a question whose Door is one-way, mixed or unknown, which the page
marks "Blind pick". A question asked
again after a Cancel answer has none: the reviewer has seen its recommendation. The
reviewer then keeps it or changes it before confirming the answer; the saved answer keeps the
first pick beside the sent choice. A comment typed with the first pick stays in the answer's
comment box, and is part of the answer only once the reviewer confirms it. Also called the blind
first pick.
_Avoid_: initial answer, draft choice

**Decision**:
A question the reviewer answered in an Explore round, with the answer kept (the latest
answer to the question's latest version): its choice, or its comment when it has no
choice, and whether the choice was the agent's recommendation ("as recommended") or not
the reviewer's first pick ("changed after your first pick"). The conclusion lists the
round's decisions.
_Avoid_: verdict, outcome

**Reset**:
Closing the current Explore round and returning to the start screen. The round's
records stay saved, but it is no longer shown or reopened, and its agent can no
longer post to it. The next round starts over: the decisions of the rounds reset
before it no longer stand, and its agent is told so.
_Avoid_: new round (which is what Start begins), clear, restart
