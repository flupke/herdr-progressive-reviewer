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
  `agent.prompt`, also while the agent is working.
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
