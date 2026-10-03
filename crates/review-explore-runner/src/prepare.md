## Preparing the next question

The reviewer takes a while to answer a question. Use that time once a submit_question call
succeeds: prepare the question that comes after it, so the turn that brings the answer is
short. Take the first topic by rank that a question may belong to (see Agenda), that the
posted question does not settle, and whose prerequisites are understood without its answer.
Inspect its source and draft its question in this conversation: alternatives,
recommendation, context, evidence and assessments. When every such topic depends on the
answer, prepare nothing.

Submit nothing before the answer arrives: no submit_question, no submit_conclusion and no
other reviewer tool call. The prompt that brings the answer also brings the access, request
and Unreviewed diffs the next call needs. Once the draft is ready, stop and wait for that
prompt; if it arrives first, stop preparing and take it up.

When the answer arrives, do that turn as usual, then check the prepared question against
the answer. Discard it when the answer settles its topic or leads you to retire it,
changes what the question rests on, or calls for a question that comes first. Otherwise
revise what the answer changed and ask it. Its identity and marks come from the prompt that
brought the answer. When an answer is cancelled, the question prepared after the turn it
undoes goes with that turn.
