# Explore evidence and MCP feedback

Slice 2 adds [durable resumption](explore-slice-2-recovery.md) to the protocol below.
Pass identity now persists independently of renewable MCP access.

The screenshot's missing Base excerpt was a viewer limitation, not evidence that
the repository changed. The comparison retained the full old text, but navigation
only searched old lines inside diff hunks. Valid ranges outside those hunks now
open full base text with old coordinates. Returning to Primary also restores it
after following another source. Old coordinates are never sent to the live LSP.

Fitting previously centered the first relevant line, which could hide the end of
a longer excerpt. Sizing and positioning now use the entire wrapped range and
its outline. Windows remain bounded and scrollable; Fit and Alt-0 return to the
chosen range after manual navigation.

Primary evidence states what it establishes and how it could change the answer.
The main evidence cycle contains only those selected excerpts. Additional question,
reply, agenda and consequence citations remain under Supporting sources, with exact
source/range duplicates collapsed. Background citations do not add yellow outlines
to the primary excerpts. The protocol requires two to five distinct alternatives for
every next question, and the reviewer appends None of the above with an open outcome.
The question sits directly above the choices. Clicking a choice or pressing its number
selects it; the text field supplements that selection. Send records both together.
There is no selectable free-text item, and None of the above can be sent without text.

The agent sends questions with `submit_question` and separate conclusions with
`submit_conclusion` through the existing reviewer MCP bridge. Conclusions separate
summary, to_be_implemented and future_work. Only the implementation list seeds the
editable task box; an explicit Implement action sends its edited contents. The UI's serial interview owner validates and
applies the turn before acknowledgement. Errors are returned to the agent while the
same request stays pending for repair. Exact accepted retries acknowledge without
replay; changed accepted payloads, cancelled work and changed agent conversations
are rejected. The current flow sends instructions and the first turn identity in
the kickoff prompt, then the agent submits its first question directly. Complete input
schemas come from MCP tool discovery, without kickoff schema examples. Later human
contributions send plain-text wakeups with the exact selected option text/ID/outcome,
optional comment, question ID/version and turn identity. The agent uses its original
question in the conversation and sends the next question with `submit_question`,
or finishes with `submit_conclusion`. Full
question records remain in the reviewer. Corrections and previous failures contribute
only the original answer ID or error when applicable; deferral is explicit.
The agent keeps its interview context in the same conversation. `get_explore_answer`,
`get_explore`, `read_explore`, source registration and the
repository catalog exchange are removed. Citations use paths, sides and ranges.
Files are read directly and Git/jj provides history. No mailbox or file fallback
remains. Existing agents need an MCP tool-catalog refresh after this change.

Follow-up delivery feedback removes Explore's one-shot send. Thread comments,
Explore turns and guide requests now share the conversation worker's pending
delivery loop and one prompting method. Busy, focused or occupied agent composers
keep a request pending until ready; cancellation drops unsent work and conversation
replacement rejects it. A reviewer no longer needs repeated Retry clicks for a
transiently busy agent.

The stale next-turn view came from gating advancement on every input event. Excluding
mouse release alone did not fix it: other activity still held back accepted questions.
Explore now selects each accepted new question regardless of intervening activity.
One question is displayed at a time, with pinned Previous/Next, Latest and Opening
controls. Earlier answers, corrections, choices, draft editors and native evidence
state remain available through history. Responses received in Files or Threads select
the new question for the next Explore visit without switching the active pane.
Duplicate acknowledgements do not change the displayed question.

Validation including shared delivery, mouse advancement, direct path citations and the
direct-answer interview flow, answer form and question history used `NEXTEST_TEST_THREADS=8 make check`: **754 passed**, **5 optional tests skipped**.
The normal sandbox blocked private endpoints with `EPERM`; the same command was
retried with access to the isolated test endpoints. The separate
real rust-analyzer Explore navigation regression passed for the preceding UI update.

UI regressions exercise historical text outside hunks, Primary after source
navigation, inherited drafts and comments on historical callers without reloading,
both ends of fitted ranges and wrapped lines, supporting navigation,
choice selection with optional details, None of the above, question placement,
narrow-terminal selection visibility, validation errors and duplicate acknowledgements. Integration
tests use a real private Herdr server and MCP HTTP client with deterministic agent
processes for Git and jj, including late session discovery, replacement and
cancellation. They submit the first question from the kickoff and read subsequent answers
directly from delivered prompts, rejecting submissions with obsolete request IDs,
wrong access values or replaced sessions. Comparison
tests verify that the metadata identifies base text through ordinary Git/jj commands.
Further integration regressions verify deferred delivery after one busy-agent request,
queued cancellation, guide replacement even if preparation fails, focus/draft
protection, session replacement and dispatcher shutdown. A UI regression
applies successive MCP turns after click/release pairs and intervening input, selects
the new question, and restores the prior draft through history. History controls stay
visible on narrow terminals while the question scrolls. Other regressions verify
background responses, duplicate acknowledgements, per-question source navigation and
replies/corrections tied to the displayed question. Unit tests also cover direct
citations, exact option/comment text, conditional correction/retry fields, and answer
payloads that stay small even when questions contain large evidence and assessments,
without repository catalogs or repeated conversation history.
They do not use live user panes. This update does not claim a new live-model reasoning
demonstration; the earlier adaptive demo remains separately documented. Relevance and
choice wording still require the agent's judgment.
