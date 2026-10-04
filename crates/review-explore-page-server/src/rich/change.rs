//! The change the rich data set cites: a review tool stops waking its agent for every reply
//! the reviewer writes, and sends the replies in batches. Three Rust files; each one's text
//! before and after the change is in `change/`, beside the diff `git diff` wrote of them (run
//! it again on two copies of the files after an edit).

use crate::changed_source::{ChangedSource, FixedChange};

/// The queue that holds the replies, and sends them.
pub(super) const QUEUE: ChangedSource = ChangedSource {
    path: "src/notify/queue.rs",
    old: include_str!("change/queue.rs.old"),
    new: include_str!("change/queue.rs.new"),
    diff: include_str!("change/queue.rs.diff"),
};

/// The rule that decides when a queue goes out.
pub(super) const FLUSH: ChangedSource = ChangedSource {
    path: "src/notify/flush.rs",
    old: include_str!("change/flush.rs.old"),
    new: include_str!("change/flush.rs.new"),
    diff: include_str!("change/flush.rs.diff"),
};

/// Where a reply is saved to its thread, then queued.
pub(super) const REPLY: ChangedSource = ChangedSource {
    path: "src/threads/reply.rs",
    old: include_str!("change/reply.rs.old"),
    new: include_str!("change/reply.rs.new"),
    diff: include_str!("change/reply.rs.diff"),
};

pub(super) const CHANGE: FixedChange = FixedChange(&[QUEUE, FLUSH, REPLY]);
