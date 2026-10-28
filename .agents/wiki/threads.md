# Review threads

Which agent gets a comment, how Herdr delivers the wakeup, and what a review access value guarantees. The terms (review thread, thread message, draft, posting, round conversation) are defined in [CONTEXT.md](../../CONTEXT.md).

## Which agent

Comments and Explore share one active-agent selection: the most recently focused live agent
in the review pane's Herdr tab, with a fallback to the tab's only agent. An agent in another
tab of the workspace is never prompted, even when it was focused last: in a herdr-bowtie
session, the lead and each task have a tab of their own in one workspace. An agent loses its
review access value, and its pin to an Explore round, when its pane moves to another tab. Herdr's `HERDR_TAB_ID`
does not follow a moved pane, so a review pane moved to another tab keeps its first tab's
agents until it restarts. Focus changes, posts, retries
and reopening use that selection; a review stores no recipient of its own. Completed answers
belong to the review, so the active agent gets only the unanswered work, with each pending
thread's full conversation as context.

The workspaces of a repository share its threads ([review state](review-state.md)), so
reviewers in several tabs load the same pending comments. Each review comment records the tab
it was posted from: loading a review, or a change of agent, wakes the agent only for the
comments its own tab posted, and `get_new_messages` returns only those, with the comments a
Retry, or reopening a thread, in this tab covered: a comment posted after that waits for its
own tab again. A comment posted from another tab shows as pending
and goes to this tab's agent only on Retry, as do comments saved before comments named their
tab. Herdr
0.9.3 names a tab `<workspace>:t<number>` and keeps both in its session snapshot, so a tab
keeps its ID across a Herdr restart (read in its source, not tried).

Each tab has its own review pane too: the open, close and toggle actions act only on the
review pane of the tab Herdr names in the action's context (Herdr 0.9.5 sends the focused
tab's `tab_id`), so a toggle in one task's tab leaves the other tabs' review panes open. A
review pane moved to another tab stays open on the next action of its first tab, which
records it for the tab it is in.

## Delivery

- Posting saves the comment, then sends the active agent a wakeup through Herdr's
  `agent.prompt`, also while the agent is working. So the wakeup does not ask the agent to
  fetch again before it ends its turn: a later comment brings its own wakeup.
- The wakeup names only the review, the access value and `get_new_messages`. The rules for
  answering come in that tool's result (`crates/review-mcp/src/instructions.rs`), those of a
  round conversation only when a fetched thread is one: the agent calls the tool anyway, so
  the rules reach it beside the threads they apply to, with no file to keep, while the
  server's MCP instructions, which every session of the agent loads, stay short.
- An agent that runs the hooks of the reviewer's Claude Code plugin (as run-ahead checks it,
  `AgentHooks::hooked`) confirms each prompt through its `UserPromptSubmit` hook: the reviewer
  types the prompt without Herdr's wait, and it counts as delivered once the hook reports the
  typed text, on the session Herdr reports. Claude Code 2.1.309 runs the hook within
  milliseconds of the submission, also for a prompt it queues while it works.
- A Codex agent confirms each prompt through the hook of the reviewer's Codex plugin, on the
  session it named last in a call to the reviewer's tools, to the thread service or the Explore
  session, when the hook recorded that it runs there
  ([ADR 0010](../adr/0010-hear-codex-s-prompts-by-session.md)). Until its first call, or on a
  session loaded before the install, Herdr's status decides. A blocked prompt runs no turn:
  Codex 0.162.1 shows "Blocked by hook" with the reason, keeps the prompt in its history and
  empties the input box.
- Text left in the agent's input box joins the typed prompt into one. While the reviewer
  waits for its prompt, the hook blocks any other prompt of that pane (of that session, for
  Codex): the joined one, whose
  delivery then fails with the text left (`PromptError::Blocked`) and waits for Retry, and
  one typed by someone else, after which the reviewer waits on for its own. A blocked prompt runs no turn; Claude
  Code 2.1.309 shows the reason and puts the prompt back in the input box, so the user clears
  the box before Retry.
- Claude Code trims the end of the prompt it reports, and wraps a long paste in
  `<pasted_content id="N">` tags, which `claude-hooks` removes before the reviewer compares.
- When the hook does not report within 5 seconds, an agent Herdr reports working or blocked
  counts as started; any other leaves the outcome unknown, since a quick turn may have ended.
- For an agent without the hooks, Herdr's status decides: the reviewer asks Herdr to wait
  until the agent is working or blocked. An agent that is working already counts at once. One
  that shows neither within Herdr's wait did not start on the prompt: its text may still wait
  in the agent's input box.
- Herdr judges by the state it knows. When an integration reports the agent's state to Herdr
  instead of Herdr reading its screen, that integration must report the turns the agent
  starts, or every prompt to an agent without the hooks counts as not started.
- Herdr owns the terminal submission and rejects a prompt to a blocked agent. The reviewer
  does not inspect the agent's terminal or its focus; it protects a prompt from the text left
  in the input box only through the hooks.

## Access values

- The wakeup gives a review access value tied to the selected agent and the review. When
  Herdr reports a native session ID, the value is bound to that session; otherwise to the
  agent's foreground process group, so wakeups and "Retry agent" still reach an agent
  without a native session ID.
- An access value is a bearer token. Each request checks that the bound pane still has the
  same native session or foreground process group; a change invalidates the grant. The
  server does not authenticate the calling process: another local caller that holds a valid
  value can use it.
