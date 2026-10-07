//! The design of the rich data set's change, in four full parts, each led by a diagram or a table
//! as the kickoff asks: a diagram of the components with new and changed nodes and a table of the
//! files; a sequence diagram, a table of the types and a state diagram, so that one part holds
//! several diagrams; a table of the costs with callouts; and a table of the rejected
//! alternatives, as long as an agent's explanation of a real change.

use review_explore::{Design, DesignPart};

pub(super) fn design() -> Design {
    Design::new(
        "Replies wait in a queue and reach the agent in one notification once the reviewer pauses or enough of them wait.",
        DesignPart::new(
            "`ReplyQueue` holds each reply in `src/notify/queue.rs` until `FlushPolicy` sends them together.",
            "\
```mermaid
flowchart LR
  reviewer(\"Reviewer\") --> thread(\"Thread file\")
  thread --> queue(\"ReplyQueue\"):::new
  policy(\"FlushPolicy\"):::new --> queue
  tick(\"Pane tick\"):::changed --> queue
  queue --> link(\"Agent link\"):::changed
  link --> agent(\"Agent\")
```

| File | What it holds | Lines |
| --- | --- | --- |
| `src/notify/flush.rs` | The rule, `FlushPolicy`, and why a batch goes out, `FlushReason` | changed +20 −4 |
| `src/notify/queue.rs` | `ReplyQueue`: the waiting replies, and the flush | changed +33 −7 |
| `src/threads/reply.rs` | Saves a reply, then queues it; flushes on close | changed +6 −3 |

The change stops telling the agent about each reply the moment the reviewer writes it. Replies \
now wait in a queue and reach the agent together, in one notification, once the reviewer \
pauses or once enough of them wait.

Nothing changes for a thread file: each reply is still saved to its thread before it is \
queued, so a crash loses no reply, only the notification about it. The agent reads a batch as \
it read a single reply before, one entry per reply, in the order the reviewer wrote them.",
        ),
        data_flow(),
        DesignPart::new(
            "A push and the idle check cost constant time, and a burst of replies costs one call to the agent instead of one each.",
            "\
| Operation | Runs | Cost now | Before |
| --- | --- | --- | --- |
| Push | Each reply | [!good] O(1): append, compare the length with the cap | One agent call |
| Idle check | Every tick, 250 ms | [!good] One subtraction of two instants | None |
| Flush | After 2 s of quiet, at 20 replies, on close | [!good] One agent call for *n* replies | *n* agent calls |
| Memory | While replies wait | [!warning] At most 20 replies | None |

A burst of twelve replies written within two seconds now costs **one** notification instead \
of twelve, and the agent starts one turn instead of queuing twelve wakeups. The pane's tick \
already runs every 250 ms to redraw its clock, so the idle check adds no thread.

> [!WARNING]
> The idle delay is measured on the pane's tick, so a flush can come up to 250 ms after the two \
seconds. A test that waits for exactly two seconds would be flaky.

> [!TIP]
> The order of replies across threads is kept: the queue is one vector for the whole pane, not \
one per thread.",
        ),
        DesignPart::new(
            "A queue per thread, a timer per push and an explicit Send all were rejected for the wakeups, threads or idle agent they bring.",
            "\
| Alternative | 12 quick replies | A lone reply | Why not |
| --- | --- | --- | --- |
| **Idle 2 s or 20 replies** (the change) | [!good] 1 agent turn | [!good] After 2 s | Kept |
| One queue per thread | [!bad] One turn per thread | [!good] After 2 s | Brings the burst back (the description) |
| A timer per push | [!good] 1 agent turn | [!warning] After 2 s, one thread per timer | The pane has no async runtime (the description) |
| Only on an explicit Send all | [!good] 1 agent turn | [!bad] When the reviewer remembers | The agent idles unseen (inferred from the issue thread) |

The last row is inferred, not stated in the code: a reviewer who forgets to press Send all \
leaves the agent idle without knowing.",
        ),
    )
}

/// The data-flow part: a sequence diagram, a table of the types, then a state diagram of the
/// queue, so that the data set shows a part with several diagrams.
fn data_flow() -> DesignPart {
    DesignPart::new(
        "A reply goes through three owners: the thread, which saves it; the queue, which holds it; and \
the agent link, which delivers the batch.",
        "\
```mermaid
sequenceDiagram
  participant reviewer as Reviewer
  participant thread as Thread file
  participant queue as ReplyQueue
  participant policy as FlushPolicy
  participant agent as Agent link
  reviewer->>thread: writes a reply
  thread->>thread: appends it and saves
  thread->>queue: push(reply, now)
  queue->>policy: is_full(waiting)?
  alt twenty replies wait
queue->>agent: notify_batch(waiting, Full)
  else fewer
queue-->>queue: keeps the reply
  end
  loop every tick of the pane
queue->>policy: is_idle(since the last push)?
policy-->>queue: true after two seconds
queue->>agent: notify_batch(waiting, Idle)
  end
  reviewer->>queue: closes the pane
  queue->>agent: notify_batch(waiting, Closing)
```

| Type | Holds | Lives | Saved |
| --- | --- | --- | --- |
| `Reply` | The thread ID and the text | From the reviewer's keystroke to the thread file | [!good] In the thread file, before it is queued |
| `ReplyQueue` | The replies the agent has not heard about, and the time of the latest push | As long as the pane | [!warning] In memory only: a crash loses the notification, not the reply |
| `FlushPolicy` | The idle delay and the size cap | A constant, `FlushPolicy::DEFAULT` | [!good] Nothing to save |
| `FlushReason` | Idle, Full or Closing | One notification | [!good] Sent with the batch |

The queue itself moves between three states, and only a flush empties it:

```mermaid
stateDiagram-v2
  [*] --> Empty
  Empty --> Waiting: push
  Waiting --> Waiting: push, fewer than twenty
  Waiting --> Empty: flush (Idle, Full or Closing)
```",
    )
}
