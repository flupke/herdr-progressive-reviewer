# Review threads

What a review thread keeps, what resolving and reading do, which agent gets a comment, how the wakeup is delivered, and what the four comment tools and their access values guarantee. The terms (review thread, thread message, draft, posting, round conversation) are defined in [CONTEXT.md](../../CONTEXT.md).

## Threads

- A review comment is posted on a selection of the diff or at the cursor. The editor keeps
  its text until posting succeeds.
- A posted message cannot be edited or deleted; a reply corrects it. An agent reply lands in
  the same thread and may address several comments.
- A draft is saved across restarts and is never sent. Cancel, or submitting empty or
  whitespace-only text, discards it. Switching between Files, Threads and Explore keeps the
  text being composed.
- The comment editor has a Vim mode and a regular mode.
- The Threads view lists the threads of the whole review, including those whose code changed
  or disappeared, filtered to Unresolved or All, with a search over messages and file paths.
- Each thread keeps its original code context. A peek at the current file gives
  highlighting, search and [language server](language-servers.md) navigation, and refreshes
  when the file changes. Going back to Files keeps the review location.

## Resolving and reading

- Resolving a thread folds its inline conversation to a summary. It does not mark the file
  reviewed.
- In Threads, resolving selects the next unresolved thread, one with unread replies first.
  When none remains, the conversation pane clears.
- Resolving stops the requests for an agent answer. A reply already on its way is saved
  without reopening the thread, and shows as unread.
- Unresolving makes the unanswered comments eligible for delivery again. Completed answers
  are never replayed.
- A file's badge shows that it has unresolved threads. An unread mark shows beside the
  filename and on the Threads tab, also for a late reply on a resolved thread.
- A reply is read once every part of it has been visible, including by scrolling through a
  reply taller than the pane.

## Which agent

Comments and Explore share one active-agent selection: the most recently focused live agent
in the Herdr workspace, with a single-agent fallback. Focus changes, posts, retries
and reopening use that selection; a review stores no recipient of its own. Pending comments
follow the selection; completed answers belong to the review, so the active agent gets only
the unanswered work, with each pending thread's full conversation as context.

## Delivery

- Posting saves the comment, then sends the active agent a wakeup through Herdr's
  `agent.prompt`, also while the agent is working. Each new comment requests one wakeup;
  a change of the agent's status or a return to the review does not repeat one already
  attempted.
- The reviewer asks Herdr to wait until the agent is working or blocked. An agent that is
  working already counts at once. One that shows neither within Herdr's 5 seconds did not
  start on the prompt: its text may still wait in the agent's prompt box.
- Herdr judges by the state it knows. When an integration reports the agent's state to Herdr
  instead of Herdr reading its screen, that integration must report the turns the agent
  starts, or every prompt counts as not started.
- Herdr owns the terminal submission and rejects a prompt to a blocked agent. The reviewer
  does not inspect the agent's terminal or its focus, and neither inspects nor protects a
  draft in its composer. Filenames and
  selected diff text are not inserted into the agent's input.
- The agent checks its pending threads through MCP, also before it finishes its work.
- A delivery error is reported and the comments stay saved. "Retry agent" on an unresolved
  thread requests another attempt without a new comment; a follow-up comment also requests
  work.
- A round conversation is delivered the same way. Its wakeup also names the round, each
  waiting message, and the question ID and version, or the stage, the message was asked
  under, with the number the Explore page showed that question by (`Shown as: Q2`) for a
  message sent from the page. It says that a message does not answer the question. The
  agent answers a round conversation with `reply`, as for any thread.
- Reopening the reviewer with pending comments sends a fresh wakeup to the active agent,
  also for comments the agent fetched but did not answer.

## The comment tools

The tools are `list_threads`, `get_thread`, `get_new_messages` and `reply`
([MCP bridge](mcp-bridge.md)). Agents read and append replies; creating and resolving a
thread stay the reviewer's actions.

- Every updated thread comes with its full conversation and original code context. A round
  conversation comes with its `explore_round`, and each of its messages with `asked_under`
  (with the question's `number`) and `quote`.
- Reading never consumes work. Each returned thread includes `in_reply_to`, the last review
  comment of that snapshot.
- `reply` takes `in_reply_to`, `message_id` and `text`. Saving the answer acknowledges the
  comments through that ID. A later comment stays pending even if it arrived before the
  answer was saved.
- A retry needs the same three values. A rejected or interrupted reply leaves the comments
  pending.
- Resolved threads are left out of pending work and of later wakeups.

## Access values

- The wakeup gives a review access value tied to the selected agent and the review. When
  Herdr reports a native session ID, the value is bound to that session; otherwise to the
  agent's foreground process group, so wakeups and "Retry agent" still reach an agent
  without a native session ID.
- Waking another agent retires the previous value for that review.
- An access value is a bearer token. Each request checks that the bound pane still has the
  same native session or foreground process group; a change invalidates the grant. The
  server does not authenticate the calling process: another local caller that holds a valid
  value can use it.
- Closing the reviewer invalidates its values.

## Storage

[Conversation store](conversation-store.md) says how threads and drafts are saved.
