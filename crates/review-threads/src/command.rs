use review_types::ReviewUnit;

use crate::{MessageId, Post, Resolution, ThreadId};

/// Review-thread work carried unchanged from the UI to the conversation owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadCommand {
    Load(ReviewUnit),
    Post {
        review_unit: ReviewUnit,
        post: Post,
    },
    Retry {
        review_unit: ReviewUnit,
        thread_id: ThreadId,
    },
    SetResolution {
        review_unit: ReviewUnit,
        thread_id: ThreadId,
        resolution: Resolution,
    },
    MarkRepliesRead {
        review_unit: ReviewUnit,
        messages: Vec<MessageId>,
    },
    MarkRead {
        review_unit: ReviewUnit,
        thread_id: ThreadId,
        through: u64,
    },
}
