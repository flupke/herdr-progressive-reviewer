My strongest recommendation is to **make Threads a first-class navigation mode alongside Files. An inline anchor should be a convenient way into a conversation—not the thing that makes the conversation accessible.**

The PR already has a good foundation: threads belong to a logical review, retain their original context, and `[c` / `]c` navigate across comments rather than only the visible diff. There is also a fallback for files that now exist only because they have threads. I would build on that rather than start with more sophisticated reanchoring. fileciteturn5file0turn6file0turn8file0

I made an **interactive mockup** of the proposed navigation:

**[Download the interactive mockup](sandbox:/mnt/data/herdr-thread-navigation.html)** · **[View the thread-screen preview](sandbox:/mnt/data/herdr-threads-preview.png)**

Open the HTML in a browser. Try marking `push_commit.rs` reviewed, then reopening its discussion through Threads. The `sync.rs` example demonstrates a conversation whose file has disappeared. Everything is sample data; sending is simulated, with no repository or agent connection.

## 1. Give Files and Threads equal standing, but different jobs

I would keep your two-pane layout and switch the contents of the sidebar:

```text
Files selected                     Threads selected
─────────────────────────────      ─────────────────────────────
File tree → Diff                    Thread list → Conversation
             ↳ Inline discussion                  ↳ Code context
```

This supports two distinct activities:

**Files:** “What code still needs reviewing?”

**Threads:** “What questions, replies, and outstanding work need my attention?”

I would **not add a permanent third pane or bottom chat panel initially**. Reusing the sidebar and main pane keeps the terminal layout manageable. Inline discussions can stay available in the diff; the standalone conversation view is another presentation of the same thread, not a separate copy.

### What the thread list should show

Each entry needs more than a filename. Show the opening question or comment, its file, its attention state, and any unusual context:

```text
model_runtime.rs                 Not sent
What is CommitPushOutcome used for?
Earlier code · File reviewed

push_commit.rs                 ● New reply
Should this retry after a corrected push?
Current code

sync.rs                        ● New reply
Do we still need the legacy fallback?
File removed
```

Use **one row per thread**, not one row per message. Your existing comment navigation can remain available for moving between individual messages.

For the first iteration, I would provide **Open / All** and a search field matching both conversation text and paths. Crucially, this list should cover the **entire logical review**, independently of the file tree’s selection, collapsed directories, reviewed status, or displayed checkpoint.

A “This file” filter can be useful, but make it explicit and removable. Otherwise the supposedly reliable way to find missing threads can itself silently hide them.

There is a useful precedent in GitHub’s Conversations menu, which exposes unresolved, resolved, and outdated conversations. The important extra contract for your reviewer is that opening a thread must work even when navigating to its code cannot. citeturn608226search12

## 2. Separate “not visible” from “no longer matches”

This is the most important distinction in your unresolved question.

In the current rendering code, a thread gets `outdated: range.is_none()`, where the range comes from placing the anchor in the presented diff. That does not distinguish failure to locate the original code from failure to represent it in this particular view. fileciteturn9file0

I would give these situations different behavior:

| Situation | Opening the thread should do |
|---|---|
| Its anchor is in the displayed diff | Show the conversation and offer a jump to the highlighted range. |
| Its code is in a folded or omitted unchanged section | Temporarily reveal the relevant context, without changing review state. |
| Its file is reviewed | Open the conversation normally; offer a read-only source peek without unreviewing the file. |
| The original selection no longer maps confidently | Show the saved original context and explain that its current location is unknown. |
| Its file is absent from the current diff, but still exists | Offer current source as well as original context. “Not in this diff” must not become “file deleted.” |
| Its file was deleted | Show the saved context and conversation; make the unavailable current-file action explicit. A verified rename can instead lead to the new path. |

**Selecting a thread should always succeed. Selecting “Go to current code” may not.**

That difference should exist in both the UI and the implementation.

### Make historical context useful, not disabled-looking

For an unplaced thread, I would show something like:

```text
What is CommitPushOutcome used for?

Open    Draft · not sent    Earlier code

┌ Original context · saved at checkpoint …
│ This selection could not be located in the current code.
│
│ CommitPushOutcome::Rejected {
│     server_version: Some(version),
│     rejected,
│ }
└                                      [Peek current source]

You
What is CommitPushOutcome used for?

[Reply]    [Resolve thread]
```

Use **“Original context”** and **“Current source”** as explicit labels. When displaying line numbers, identify the historical checkpoint and diff side rather than making old numbers look current.

You already retain the excerpt and original anchor content, so there is a basis for this view even when the working file is gone. fileciteturn5file0

A current-source peek should **not imply that the thread has been reattached there**. Preserve the original context; treat any current location as a separate mapping. For uncertain matches, an honest “original location unavailable” is preferable to confidently highlighting unrelated code.

## 3. Keep review progress, conversation completion, and delivery separate

I would make the distinctions visible through behavior:

| State | Meaning |
|---|---|
| **File reviewed** | I inspected this version of the file. |
| **Thread resolved** | This question or concern is settled. |
| **Agent answered** | A response arrived—not necessarily a satisfactory one. |
| **New reply** | There is a response I have not read. |

In particular, **marking a file reviewed must not resolve or hide its conversations**.

I would let the existing review action continue without a blocking dialog, then show a small acknowledgment:

> File reviewed. 2 open threads remain.

Keep the thread badge on the reviewed file, and keep those conversations in Threads. Conversely, resolving a thread should not mark its file reviewed.

An agent response should also not silently resolve the thread. The user may need to inspect the resulting code or ask a follow-up. Likewise, a reply arriving on a reviewed file should create a new-reply indication, not itself revoke the file’s reviewed state; actual code changes are a separate reason to require another pass.

### Expose both kinds of unfinished work

For example:

```text
17/34 files reviewed · 4 open threads · 2 new replies

1 unsent · 1 waiting                     Ctrl-s · Send 1 draft
```

Those counts represent different things. An open thread can be waiting, answered, or contain an unsent reply. Avoid presenting them as interchangeable “pending” totals.

When all files are reviewed, the screen should still communicate:

> All current changes reviewed. 4 conversations remain open.

That is much better than an empty diff pane that suggests the whole review is finished.

For incoming answers, update the badge without changing focus or scrolling the diff. Keep thread-list ordering stable while the user navigates it; reading an answer should not make the next item jump under their cursor. A global “New replies” entry should also expose late replies on resolved threads, rather than letting them disappear into history.

## 4. The screenshot: the UI changes I would prioritize

### Make the save/send boundary unambiguous

This is the biggest wording problem.

The current behavior is that **Submit / Ctrl-Enter saves locally**, while **Ctrl-s sends pending comments to the selected agent’s queue**. The implementation also saves an active editor before sending. fileciteturn2file0turn8file0

I would change the visible language:

| Current wording | Proposed wording |
|---|---|
| `Submit` | **Save draft** |
| `queued locally` | **Draft · not sent** |
| `Ctrl-Enter save` | **Ctrl-Enter · Save draft** |
| `Ctrl-s send` | **Ctrl-s · Send 3 drafts to ‹session›** |

“Queued” sounds as though delivery will happen automatically. “Draft · not sent” makes the remaining action clear.

When an editor is open, be equally explicit about the scope of Ctrl-s: **save this edit and send all drafts**, not merely “send this comment.” The destination matters too, particularly when the selected agent session can change.

Keep uncertain delivery separate from an ordinary unsent draft. Your model already distinguishes `Uncertain` and deliberately avoids automatic resubmission; preserve that caution in the UI with an inspection action, rather than making every problem look like a generic Retry. fileciteturn5file0

### Opening a conversation should not immediately mean editing it

Currently, clicking an unanswered comment edits it, while clicking an answered one opens a blank follow-up. That makes the same click behave differently depending on an asynchronous event. fileciteturn6file0

I would use:

**Select/click → read or focus.**  
**`A` / Reply → compose a follow-up.**  
**`e` / Edit draft → edit an unsent message.**

The full composer should appear when requested. A lightweight `Reply…` affordance can remain visible, but merely revisiting a conversation should not open a large empty input.

This is especially valuable once users enter through a thread index: they may be checking an answer, not preparing another message.

### Give the comment stronger hierarchy than the raw diff wrapper

In your screenshot, the historical diff occupies most of the first card, while the actual question appears below it. The very dim source also looks more like disabled content than useful historical evidence.

I would put the **question first**, followed by clearly labeled context. Show a short excerpt initially—perhaps four to eight lines—and allow expansion. Keep that excerpt readable.

The `diff --git`, repeated paths, and hunk metadata can move into expanded context or a copy action. The file path and checkpoint belong in the context header, not repeatedly inside the main conversation.

Historical context should look **historical**, not **unusable**.

### Reduce competing borders and mode labels

The screenshot has several nested frames around the thread and editor. I would reserve the strongest border or highlight for the actual keyboard focus, use a subtler context frame, and separate messages with simple rules.

The editor also shows both `[vim] normal` and `Vim: Insert`. Whatever the intended distinction, they look like competing reports of the active mode. Prefer a single explicit display:

```text
Vim · INSERT                                 F2 · Regular editing
```

Keep editor instructions local to the editor, navigation instructions in the pane footer, and send-to-agent controls in the review footer.

### Give review guides and conversations different markers

One detail from the code is particularly relevant: the current file-tree `💬` is driven by `guide_paths`, and is suppressed on reviewed files. It is not a persistent thread-count badge. fileciteturn12file0

I would not extend that same marker’s meaning to cover both guides and conversations. Give threads a separate, consistently positioned indicator, for example:

```text
✓ model_runtime.rs                T1
○ push_commit.rs               G  T2 ●
```

The exact symbols can change, but the semantics should not: review guide, open-thread count, new reply.

Reserve space for these indicators before truncating filenames. In the deeply nested tree shown in the screenshot, compacting single-child directory chains would also give filenames and status more breathing room. Collapsed directories should aggregate thread/new-reply indicators so that collapsing a folder does not hide the existence of discussion.

## 5. Simplify the conversation’s editing lifecycle

There is another UX decision worth making now: **once a message has been delivered, should it still be editable?**

The PR currently permits editing until an answer arrives, while retaining the submitted version and protecting edits that race with an answer. That handles difficult races, but it leaves the UI explaining two versions of “your comment.” fileciteturn5file0

My preference would be:

> **Unsent drafts are editable. Sent messages are immutable. Corrections become follow-ups.**

That makes “what did the agent actually receive?” straightforward. It also means delivery progress cannot suddenly change the meaning of an editor the user has open.

This does not need to block the navigator work, but it would simplify the eventual interaction model.

Similarly, deleting sent history should never resemble cancelling the agent’s work. Your README already warns that local removal does not cancel queued work. I would surface that distinction at the destructive action itself, and make Resolve the normal way to clear a completed conversation from the active list. fileciteturn2file0

## 6. How I would fit this into the current implementation

The key architectural change is to make **thread selection independent of file selection**.

You already have stable `ThreadId`s and review-scoped storage. At present, the conversation UI lives within the diff component, and comment navigation goes through a file-selection request. A standalone view should be able to render directly from the thread, even without a corresponding loaded document. fileciteturn5file0turn6file0

Conceptually:

```text
ReviewThreads, scoped to ReviewUnit
    ├── Threads navigator
    ├── File/directory badges
    └── Conversation view selected by ThreadId

Current documents + anchor mapping
    └── Optional inline placement / jump-to-code destination
```

I would share the message and composer rendering between inline and standalone views, rather than duplicate conversation state.

For anchoring, separate **mapping information** from **visibility information**. A thread can have a valid current mapping while its range is absent from the displayed diff. Derive those properties from the current revision and view; do not store “outdated” as a permanent attribute of the conversation.

Resolution and read state, by contrast, should be durable. A small explicit resolution field and a last-seen reply/event marker would support the proposed Open and New reply views.

### What I would ship first

| Increment | Scope |
|---|---|
| **Reliable access** | Files / Threads switch; review-wide thread list; standalone detail using saved context; return to the previous review location. |
| **Clear workflow** | Save draft / Send wording; persistent thread badges; explicit Resolve and unread-reply state. |
| **Better context** | Temporary source peeks, precise placement explanations, compact inline summaries, and only then improved reanchoring. |

For navigation back to the diff, preserve the selected file, checkpoint, cursor, scroll, and focus. Opening a historical conversation should not move the review baseline or silently change filters.

The acceptance tests I would emphasize are behavioral: create a thread and mark its file reviewed; remove its file while an answer is pending; hide its range through folding or filtering; switch reviews and restart; finish reviewing every file. In every case, the conversation must remain findable, its original context must remain correct, and accessing it must not unexpectedly modify review state or discard a draft.

**I would ship the independent thread navigator before smarter reanchoring.** Better anchoring makes conversations easier to reach from code. The navigator guarantees that they remain reachable when there is no longer any code to reach them from.

---

If you want, I can:

- Outline next steps for implementing independent thread navigation
- Draft a sample UI specification for the thread list interface
- Describe user interactions transitioning from file-based review to thread-based review