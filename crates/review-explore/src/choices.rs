//! Every question has a reviewer-owned escape from its proposed alternatives.
use crate::{Alternative, Question, TopicStatus};
use std::sync::LazyLock;

static NONE_OF_THE_ABOVE: LazyLock<Alternative> = LazyLock::new(|| Alternative {
    id: "none-of-the-above".into(),
    text: "None of the above".into(),
    outcome: TopicStatus::Open,
    recommendation: None,
});

impl Question {
    /// Displayed choices include the built-in alternative without rewriting posted wording.
    pub fn choices(&self) -> impl Iterator<Item = &Alternative> {
        self.alternatives
            .iter()
            .chain(std::iter::once(&*NONE_OF_THE_ABOVE))
    }
}
