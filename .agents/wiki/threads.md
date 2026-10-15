# Review threads

Which agent gets a comment, how Herdr delivers the wakeup, and what a review access value guarantees. The terms (review thread, thread message, draft, posting, round conversation) are defined in [CONTEXT.md](../../CONTEXT.md).

## Which agent

Comments and Explore share one active-agent selection: the most recently focused live agent
in the Herdr workspace, with a single-agent fallback. Focus changes, posts, retries
and reopening use that selection; a review stores no recipient of its own. Pending comments
follow the selection; completed answers belong to the review, so the active agent gets only
the unanswered work, with each pending thread's full conversation as context.

## Delivery

- Posting saves the comment, then sends the active agent a wakeup through Herdr's
  `agent.prompt`, also while the agent is working. So the wakeup does not ask the agent to
  fetch again before it ends its turn: a later comment brings its own wakeup.
- The wakeup names only the review, the access value and `get_new_messages`. The rules for
  answering come in that tool's result (`crates/review-mcp/src/instructions.rs`), those of a
  round conversation only when a fetched thread is one: the agent calls the tool anyway, so
  the rules reach it beside the threads they apply to, with no file to keep, while the
  server's MCP instructions, which every session of the agent loads, stay short.
- The reviewer asks Herdr to wait until the agent is working or blocked. An agent that is
  working already counts at once. One that shows neither within Herdr's 5 seconds did not
  start on the prompt: its text may still wait in the agent's prompt box.
- Herdr judges by the state it knows. When an integration reports the agent's state to Herdr
  instead of Herdr reading its screen, that integration must report the turns the agent
  starts, or every prompt counts as not started.
- Herdr owns the terminal submission and rejects a prompt to a blocked agent. The reviewer
  does not inspect the agent's terminal or its focus, and neither inspects nor protects a
  draft in its composer.

## Access values

- The wakeup gives a review access value tied to the selected agent and the review. When
  Herdr reports a native session ID, the value is bound to that session; otherwise to the
  agent's foreground process group, so wakeups and "Retry agent" still reach an agent
  without a native session ID.
- An access value is a bearer token. Each request checks that the bound pane still has the
  same native session or foreground process group; a change invalidates the grant. The
  server does not authenticate the calling process: another local caller that holds a valid
  value can use it.
