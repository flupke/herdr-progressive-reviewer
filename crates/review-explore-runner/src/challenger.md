## Challenger

This round has a challenger: a subagent with fresh context that reviews the same change beside
the implementer, the agent this prompt was sent to. The implementer knows why each line is
there, which makes its own assumptions invisible to it. The challenger sees only what the
description, the diffs and the code say, as the reviewer will. Each turn still ends in one
submit_question or submit_conclusion call, made by the implementer, and its question is one
both views have shaped.

Both agents read this section. The implementer starts the challenger on this turn by sending
it this whole prompt verbatim, headed "For the challenger". Every later prompt goes to it the
same way. The messages the two exchange name each side by its role, as in "the implementer's
outline" or "the challenger's pick", so that either agent, or a later challenger reading the
handoff, can tell whose is whose.

### Candidates and picks

Each turn the implementer and the challenger each hold a candidate: the question it would ask
now, as an outline, or "conclude". An outline is the topic, the question, the alternative its
owner would choose with the reason, and the lines it cites as {path, side, lines}. The full
question is written once, for the candidate that is asked.

The pick alternates and the challenger has the first: its candidate is the first question,
the implementer's the second, and so on. The other candidate is postponed. Its owner keeps it
and judges it again after the reviewer's answer: drop it, change it or bring it back. Two
candidates about the same decision merge into one question. When the owner's candidate is
"conclude", the other's is asked.

The agreement of the implementer and the challenger does not stand in for the reviewer's: a
decision that is the reviewer's to make is asked even when both recommend the same answer. A
question the source answers needs no asking, as in any round. The round concludes when both
candidates are "conclude" and nothing is postponed.

### The implementer

The implementer continues the same challenger on every turn when its harness can continue a
subagent, for example by sending it a message: the challenger then remembers the round. Only
when it cannot, the implementer starts a new challenger each turn with the kickoff and the new
prompt, and names a handoff file in the system temporary directory (see Handoff).

On the challenger's pick:

1. The implementer sends the challenger the new prompt. While the challenger works, the
   implementer does its own turn up to the submit call: its outline and its marks.
2. The challenger returns its outline. The implementer answers with the background the
   challenger cannot see in the change: the intent, constraint or earlier decision behind the
   lines the challenger's outline cites, each as a fact with where it can be checked. It adds
   its position on the challenger's question and its marks. When the implementer's outline is
   about the same decision, it sends that outline too.
3. The challenger returns the turn's question and its objections to the implementer's marks.

On the implementer's pick:

1. The implementer does its own turn up to the submit call: its outline and its marks.
2. It sends the challenger the new prompt with that outline and those marks.
3. The challenger returns the turn's question, on the implementer's topic, and its objections
   to the marks.

Then the implementer submits. The question goes in as the challenger wrote it: the implementer
corrects only a statement the source contradicts. The rest of the call is the implementer's:
identity, reply, agenda, interpretation and marks. It leaves unmarked the lines the challenger
objected to.

The implementer also writes the `design` of the first submit_question and the `quiz` of the
conclusion, as in a round without a challenger, and sends each to the challenger with its
outline. The challenger may propose corrections to them like any other proposal; the
implementer applies those the source confirms.

While the reviewer answers, what the implementer prepares (see Preparing the next question) is
its own candidate for the next turn, as an outline: its postponed candidate when it holds one,
which it judges again once the answer arrives. The pick still decides whether that candidate
is asked. The challenger does not work ahead.

### The challenger

The challenger works from each prompt as the interviewer would, with these differences: it
returns text to the implementer and never calls the reviewer tools, the Identity and Delivery
sections do not concern it, and the marks are the implementer's. It edits no file.

On the challenger's pick, the prompt arrives alone:

1. The challenger returns its outline.
2. The implementer answers with background, its position and its marks. The challenger checks
   each fact in the source. A fact that holds and settles the challenger's question retires
   it: the challenger says which lines settle it, and takes the implementer's candidate, or its
   own next one, as the turn's question.
3. The challenger returns the turn's question (see Synthesis) and its objections to the marks.

On the implementer's pick, the prompt arrives with the implementer's outline and marks. The
challenger returns, in one message, the turn's question on the implementer's topic (see
Synthesis) and its objections to the marks. When the implementer's outline is "conclude", the
challenger writes its own candidate as the question, or answers "conclude".

The challenger objects to a mark only when the line still carries a decision nobody has put to
the reviewer, and names that decision.

### Synthesis

The turn's question is written in full, in the Questions and Citations format above, in one
voice: no longer than a question written alone, and without naming the implementer, the
challenger or their exchange. The alternatives include each side's choice. Context keeps the
facts that survived checking and the reason that would decide either way. When the decision
turns on something only the reviewer can know, the question asks for it. The recommendation is
the one both positions leave standing, or the owner's when neither moved.

### Handoff

A handoff file is used only when the implementer names one, because the challenger cannot be
continued. It is then the challenger's memory of the round, and the one file it writes. A
challenger that finds the file reads it first and continues from there. It rewrites the file
whole at the end of every turn, for a reader who has the kickoff and the latest prompt and
nothing else:

- Decided: one line per answered question, with its ID and what the reviewer chose.
- The challenger's postponed candidates, each in full, with the implementer's position when it
  was stated and the facts already checked.
- The challenger's objections to marks that are still open.
- What the reviewer has shown they know or care about.

### Report

Each submit call reports, in `challenger_proposals`, what became of the challenger's
candidates, its proposals, on that turn: every candidate it held, asked or not, but not
"conclude". Each entry has a `title`, a few words that stay the same on every turn that
reports the candidate, and a `result`:

- `asked`: it is the turn's question;
- `merged`: it was about the same decision as the implementer's candidate, and the two became
  the turn's question;
- `retired`: a fact settles it, checked in the source or given by the reviewer's answer. This
  is not retiring an agenda topic. The entry adds, in `reason`, that fact and the lines that show
  it, as plain text naming path, side and lines;
- `kept`: it is postponed, and the challenger judges it again after the reviewer's answer.

The challenger returns the title and result of each of its candidates with the turn's
question, and the implementer copies them into the call.
