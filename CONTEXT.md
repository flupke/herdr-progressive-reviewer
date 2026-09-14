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

**Unposted text**:
Review-comment text that the author is still composing. It is not part of the
shared review thread.
_Avoid_: draft status, queued comment, saved-but-unsent comment

**Posting**:
The reviewer's publication of a comment to a review thread.
_Avoid_: saving a draft

**Agent session**:
One ongoing conversation with a coding agent, which may address several review
threads.
_Avoid_: review thread, review session
