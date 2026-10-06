# Explore round

What an Explore round guarantees the reviewer, and why, from Start to Reset: its scope, the unchanged code it assumes, what its judgements and recaps do not establish, when its review marks move, and what Implement and Reset do. The terms are defined in [CONTEXT.md](../../CONTEXT.md).

Related pages: what the agent's prompts and tools guarantee is in [Explore agent contract](explore-agent-contract.md); the saved round in [Explore recovery](explore-recovery.md); the settings in [Explore settings](explore-settings.md). Explore is experimental.

## Scope

- A round covers the complete change from the base to the working copy, including files
  already reviewed and files the reviewer filtered out.
- The agent keeps the round's context in its own agent session. A wakeup carries the answer
  and its identities, not the history, so a newly selected agent session may need the
  reviewer to supply what it lacks.

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
- Request and answer identities protect against cancelled, duplicate and unrelated
  responses. They do not establish that the source is fresh.

## What the agent's judgements are

- Door and Blast radius are the agent's judgements, backed by evidence; neither is a risk
  score or a guarantee.
- The recap shows the agent's interpretation of the answer. A factual question or added
  context is not agreement. The interpretation of free text is something to check and to
  amend with another answer, not a semantic guarantee.
- The yellow outline of a cited range means "relevant to this question", not accepted,
  reviewed or high risk.

## Review marks from the answers

- Review progress moves when the reviewer acts. The marks that come with a question wait
  until the reviewer answers it, so the lines an answer settled are marked when the
  reviewer answers the next question, or at once when the agent concludes instead.
- Citations and topic associations mark nothing. Explore resolves no review thread.
- Cancel answer gives back the review marks of the agent's turn after the answer. When the
  code changed since the round started, lines the cancelled turn reopened stay open.
- An answer cannot be cancelled once Implement was sent.

## The conclusion and Implement

- Implement authorizes the agent to implement exactly the list in the box. Summary and
  Future work are not added to the request. Reply authorizes no code change.
- Reopening sends no prompt and never starts an implementation. A delivered request is not
  sent again by itself.
- "Delivery outcome unknown" means that a crash or a transport failure may have interrupted
  the confirmation: the reviewer checks the agent's session before sending a new
  request on purpose.

## Reset

- The next round's kickoff lists none of the reset rounds' decisions and tells the agent
  that they are void, even when its agent session still holds them.
- The review marks that the reset rounds applied stay.
