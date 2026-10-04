use std::sync::Arc;

use review_source::DiffRangeAnchor;
use serde::{Deserialize, Serialize};

use crate::{ReviewThread, ReviewThreads};

/// Immutable original code, shared by conversation snapshots and stored separately.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThreadSource {
    pub anchor: DiffRangeAnchor,
    pub excerpt: String,
}

/// What a review thread discusses: a code selection, or one Explore round.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ThreadSubject {
    /// The round conversation of the Explore round with this instance.
    Round {
        round: String,
    },
    Code(Arc<ThreadSource>),
}

impl ReviewThread {
    /// The code selection the thread discusses; a round conversation has none.
    pub fn code(&self) -> Option<&ThreadSource> {
        match &self.subject {
            ThreadSubject::Code(source) => Some(source),
            ThreadSubject::Round { .. } => None,
        }
    }

    /// Path that originally held the selected source; a round conversation has none.
    pub fn path(&self) -> Option<&str> {
        let anchor = &self.code()?.anchor;
        Some(
            anchor
                .new_path
                .as_deref()
                .or(anchor.old_path.as_deref())
                .unwrap_or(""),
        )
    }
}

impl ReviewThreads {
    /// The source of each thread on code, in thread order.
    pub fn sources_mut(&mut self) -> impl Iterator<Item = &mut Arc<ThreadSource>> {
        self.threads
            .iter_mut()
            .filter_map(|thread| match &mut thread.subject {
                ThreadSubject::Code(source) => Some(source),
                ThreadSubject::Round { .. } => None,
            })
    }
}
