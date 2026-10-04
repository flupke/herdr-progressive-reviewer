//! The change the rich data set cites: a review tool stops waking its agent for every reply
//! the reviewer writes, and sends the replies in batches. Three Rust files that the questions
//! cite; each one's text before and after the change is in `change/`, beside the diff
//! `git diff` wrote of them (run it again on two copies of the files after an edit). Around
//! them, as in a real change of that size, forty files that change one line each, most of them
//! under one deep directory, so that the meter lists long paths that start alike. They come
//! after the three, by path, as the review tool lists a change's files.

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

/// A file of the change whose one changed line turns batching on.
macro_rules! batching_on {
    ($path:literal) => {
        ChangedSource {
            path: $path,
            old: "batching: off\n",
            new: "batching: on\n",
            diff: concat!(
                "diff --git a/",
                $path,
                " b/",
                $path,
                "\n--- a/",
                $path,
                "\n+++ b/",
                $path,
                "\n@@ -1 +1 @@\n-batching: off\n+batching: on\n",
            ),
        }
    };
}

pub(super) const CHANGE: FixedChange = FixedChange(&[
    QUEUE,
    FLUSH,
    REPLY,
    batching_on!(".github/workflows/notify-batching.yml"),
    batching_on!("CHANGELOG.md"),
    batching_on!("Cargo.toml"),
    batching_on!("README.md"),
    batching_on!("docs/configuration.md"),
    batching_on!("docs/notifications/batching.md"),
    batching_on!("docs/usage.md"),
    batching_on!("src/lib.rs"),
    batching_on!("src/notify/delivery/batching/batch_id.rs"),
    batching_on!("src/notify/delivery/batching/batch_store.rs"),
    batching_on!("src/notify/delivery/batching/cap_policy.rs"),
    batching_on!("src/notify/delivery/batching/clock.rs"),
    batching_on!("src/notify/delivery/batching/deduplicate.rs"),
    batching_on!("src/notify/delivery/batching/delivery_error.rs"),
    batching_on!("src/notify/delivery/batching/digest.rs"),
    batching_on!("src/notify/delivery/batching/flush_reason.rs"),
    batching_on!("src/notify/delivery/batching/metrics.rs"),
    batching_on!("src/notify/delivery/batching/mod.rs"),
    batching_on!("src/notify/delivery/batching/ordering.rs"),
    batching_on!("src/notify/delivery/batching/pane_close.rs"),
    batching_on!("src/notify/delivery/batching/queue_limits.rs"),
    batching_on!("src/notify/delivery/batching/quiet_window.rs"),
    batching_on!("src/notify/delivery/batching/quote.rs"),
    batching_on!(
        "src/notify/delivery/batching/replies_sent_after_the_pane_closed_join_the_last_batch.rs"
    ),
    batching_on!("src/notify/delivery/batching/retry_backoff.rs"),
    batching_on!("src/notify/delivery/batching/settings.rs"),
    batching_on!("src/notify/delivery/batching/tests/cap_policy.rs"),
    batching_on!("src/notify/delivery/batching/tests/ordering.rs"),
    batching_on!("src/notify/delivery/batching/tests/pane_close.rs"),
    batching_on!("src/notify/delivery/batching/tests/quiet_window.rs"),
    batching_on!("src/notify/delivery/batching/tests/retry_backoff.rs"),
    batching_on!("src/notify/delivery/batching/thread_index.rs"),
    batching_on!("src/notify/delivery/batching/wakeup_prompt.rs"),
    batching_on!("src/notify/delivery/batching/wakeup_target.rs"),
    batching_on!("src/notify/delivery/mod.rs"),
    batching_on!("src/notify/delivery/sender.rs"),
    batching_on!("src/runtime/effects.rs"),
    batching_on!("src/settings.rs"),
    batching_on!("src/threads/post.rs"),
    batching_on!("src/threads/wakeup.rs"),
]);
