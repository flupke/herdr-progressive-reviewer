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
A discussion about a code selection within one review. It remains part of the
review when the selected code changes or disappears.
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
reviewer could explain the change at a whiteboard: what it adds and where, its types
and data flow, its algorithm and cost, and the alternatives the implementer rejected.
_Avoid_: map (the provisional map of the first reply, which it replaces)

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

**Cancel answer**:
Withdrawing the reviewer's latest answer in an Explore round, which returns the
round to its state before that answer, including the review marks it led to.
_Avoid_: cancel (which stops a pending turn), correction, undo, defer

**Explore page**:
A browser page, served by the open review pane, that shows the current state of
its Explore round: no round, a round starting or failing to start, the agent
working, the agent's question with its choices, a turn the agent is no longer
working on, or the conclusion with its quiz. The reviewer can start a round,
answer the question and implement the conclusion there, as in the pane. Its
address carries a token, and the page refuses requests without it. The pane
serves it on this machine, for the Herdr action and for Start and Start with
Challenger, which open it in the browser (Start in the pane does not), and on the
network, behind a new
token for each round, for the QR code in the pane; while no round runs, the start
screen's token is the one the next round keeps.
_Avoid_: web UI, Explore web, browser view

**Round on the page**:
An Explore round started with Start or Start with Challenger, or on the Explore
page. The pane shows only that it runs on the page, where it stands, and the
page's address, until the reviewer chooses Continue in the pane. A round started
with Start in the pane is a round in the pane, which shows its interview.
_Avoid_: web session, web round

**First pick**:
The choice the reviewer picks on the Explore page before it shows the agent's
recommendation, on a question whose Door is one-way, mixed or unknown. A question asked
again after a Cancel answer has none: the reviewer has seen its recommendation. The
reviewer then keeps it or changes it before sending; the saved answer keeps the
first pick beside the sent choice. Also called the blind first pick.
_Avoid_: initial answer, draft choice

**Reset**:
Closing the current Explore round and returning to the start screen. The round's
records stay saved, but it is no longer shown or reopened, and its agent can no
longer post to it.
_Avoid_: new round (which is what Start begins), clear, restart
