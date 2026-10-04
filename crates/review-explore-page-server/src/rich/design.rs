//! The design of the rich data set's change, in four full parts, with a sequence diagram, a
//! table with status marks and callouts, as long as an agent's explanation of a real change.

use review_explore::{Design, DesignPart};

pub(super) fn design() -> Design {
    Design::new(
        "Replies wait in a queue and reach the agent in one notification once the reviewer pauses or enough of them wait.",
        DesignPart::new(
            "`ReplyQueue` holds each reply in `src/notify/queue.rs` until `FlushPolicy` sends them together.",
            "\
The change stops telling the agent about each reply the moment the reviewer writes it. Replies \
now wait in a queue, `ReplyQueue`, and reach the agent together, in one notification, once the \
reviewer pauses or once enough of them wait.

Three files move. `src/notify/flush.rs` gains the rule that decides when a queue goes out \
(`FlushPolicy`) and the reasons a notification can carry (`FlushReason`). \
`src/notify/queue.rs` keeps the waiting replies and sends them. `src/threads/reply.rs`, which \
saves a reply to its thread, now queues it, and sends what is left when the pane closes its \
threads.

Nothing changes for a thread file: each reply is still saved to its thread before it is \
queued, so a crash loses no reply, only the notification about it. The agent reads a batch as \
it read a single reply before, one entry per reply, in the order the reviewer wrote them.",
        ),
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
| `FlushReason` | Idle, Full or Closing | One notification | [!good] Sent with the batch |",
        ),
        DesignPart::new(
            "A push and the idle check cost constant time, and a burst of replies costs one call to the agent instead of one each.",
            "\
A push appends to a vector and compares its length with the cap: constant time. The pane's \
tick, which already runs every 250 ms to redraw its clock, asks the queue whether the reviewer \
has been idle long enough; that is one subtraction of two instants.

A flush makes one call to the agent link with every waiting reply, where the old code made one \
call per reply. A burst of twelve replies written within two seconds now costs one notification \
instead of twelve, and the agent starts one turn instead of queuing twelve wakeups.

Memory grows with the replies waiting, at most twenty, since the twentieth push sends the \
queue.

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
**One queue per thread.** The change's description rejects it: the agent would get one \
notification per thread after the same pause, which brings back the burst of wakeups the change \
removes.

**A timer per push**, cancelled by the next push. Rejected in the description too: the pane \
has no async runtime, and a thread per timer for a two-second delay costs more than the tick it \
already has.

**Flushing only on an explicit Send all.** Inferred from the issue thread, not stated in the \
code: a reviewer who forgets to press it leaves the agent idle without knowing.",
        ),
    )
}
