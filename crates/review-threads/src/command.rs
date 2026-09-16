use review_types::ReviewUnit;

use crate::{Draft, DraftTarget, MessageId, Post, Resolution, ThreadId};

/// Review-thread work carried unchanged from the UI to the conversation owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadCommand {
    Load(ReviewUnit),
    SaveDraft {
        review_unit: ReviewUnit,
        draft: Draft,
    },
    DiscardDraft {
        review_unit: ReviewUnit,
        target: DraftTarget,
    },
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
