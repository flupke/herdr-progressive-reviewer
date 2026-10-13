//! The design of the rich data set's change, in four full parts written as the kickoff asks, in
//! plain words with no file, type or function name: the story and a diagram of the components with
//! new and changed nodes; a sequence diagram, a table of the data and a state diagram, so that one
//! part holds several diagrams; a table of the costs with callouts; and a table of the rejected
//! alternatives.

use review_explore::{Design, DesignPart};

pub(super) fn design() -> Design {
    Design::new(
        "Replies wait in a queue and reach the agent in one notification once the reviewer pauses or enough of them wait.",
        DesignPart::new(
            "A reply queue holds each reply until the flush rule sends the waiting replies together.",
            "\
1. Before, each reply the reviewer wrote woke the agent at once.
2. A burst of replies started as many agent turns, which queued up behind each other.
3. Now the replies wait, and reach the agent together once the reviewer pauses.
4. The description gives the reason: one turn for a burst instead of one per reply.

```mermaid
flowchart LR
  reviewer(\"Reviewer\") --> thread(\"Thread file\")
  thread --> queue(\"Reply queue\"):::new
  policy(\"Flush rule\"):::new --> queue
  tick(\"Pane clock\"):::changed --> queue
  queue --> link(\"Agent link\"):::changed
  link --> agent(\"Agent\")
```

The reply queue sits between the thread file and the agent link. The pane's clock asks the \
flush rule, on each tick, whether the waiting replies go out. Each reply is still saved before \
it waits, so a crash loses a notification, never a reply.",
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
  participant queue as Reply queue
  participant policy as Flush rule
  participant agent as Agent link
  reviewer->>thread: writes a reply
  thread->>thread: appends it and saves
  thread->>queue: adds the reply
  queue->>policy: are twenty waiting?
  alt twenty replies wait
queue->>agent: sends the batch, full
  else fewer
queue-->>queue: keeps the reply
  end
  loop every tick of the pane
queue->>policy: two seconds since the last reply?
policy-->>queue: yes
queue->>agent: sends the batch, idle
  end
  reviewer->>queue: closes the pane
  queue->>agent: sends the batch, closing
```

| Data | Holds | Lives | Saved |
| --- | --- | --- | --- |
| A reply | Its thread and its text | From the reviewer's keystroke to the thread file | [!good] In the thread file, before it waits |
| The reply queue | The replies the agent has not heard about, and the time of the latest one | As long as the pane | [!warning] In memory only: a crash loses the notification, not the reply |
| The flush rule | The idle delay and the size cap | Fixed in the code | [!good] Nothing to save |
| A batch's reason | Idle, full or closing | One notification | [!good] Sent with the batch |

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
