use std::ops::Deref;

use review_guide::DiffRangeAnchor;
use serde::{Deserialize, Serialize};

use crate::{ReviewThread, ReviewThreads};

/// Immutable original code, shared by conversation snapshots and stored separately.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadSource {
    pub anchor: DiffRangeAnchor,
    pub excerpt: String,
}

impl Deref for ReviewThread {
    type Target = ThreadSource;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl<S> ReviewThreads<S> {
    /// Exchange loaded context for durable references (or hydrate those references)
    /// without copying or changing any conversation history or attention state.
    pub fn try_map_sources<T, E>(
        self,
        mut map: impl FnMut(S) -> Result<T, E>,
    ) -> Result<ReviewThreads<T>, E> {
        let threads = self
            .threads
            .into_iter()
            .map(|thread| {
                Ok(ReviewThread {
                    id: thread.id,
                    source: map(thread.source)?,
                    messages: thread.messages,
                    resolution: thread.resolution,
                    seen_reply_through: thread.seen_reply_through,
                    seen_replies: thread.seen_replies,
                })
            })
            .collect::<Result<_, E>>()?;
        Ok(ReviewThreads {
            review_unit: self.review_unit,
            drafts: self
                .drafts
                .into_iter()
                .map(|draft| draft.try_map_source(&mut map))
                .collect::<Result<_, E>>()?,
            threads,
            sequence: self.sequence,
            readers: self.readers,
            answered: self.answered,
        })
    }
}
