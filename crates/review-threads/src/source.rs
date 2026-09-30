use std::ops::Deref;
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

impl Deref for ReviewThread {
    type Target = ThreadSource;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl ReviewThreads {
    /// Each thread's source, in thread order.
    pub fn sources_mut(&mut self) -> impl Iterator<Item = &mut Arc<ThreadSource>> {
        self.threads.iter_mut().map(|thread| &mut thread.source)
    }
}
