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

    /// The displayed choice whose ID is `id`.
    pub fn choice(&self, id: &str) -> Option<&Alternative> {
        self.choices().find(|choice| choice.id == id)
    }
}

impl Alternative {
    /// Whether this is the built-in None of the above, which the reviewer explains in words.
    pub fn is_none_of_the_above(&self) -> bool {
        self.id == NONE_OF_THE_ABOVE.id
    }

    /// The parenthesised mark that ends the choice's text when the text says that the choice is
    /// recommended: "(Recommended)", or "(Recommended: ...)" with a reason after a punctuation
    /// mark. "(recommended by ...)" names someone else's advice and is no such mark. The
    /// recommendation field says it, and a question that hides it until the reviewer's first
    /// pick must not show it in the text.
    pub(crate) fn recommended_mark(&self) -> Option<&str> {
        const MARK: &str = "recommended";
        let text = self.text.trim_end().trim_end_matches(['.', '!']).trim_end();
        let open = text.strip_suffix(')')?.rfind('(')?;
        let inside = text[open + 1..text.len() - 1].trim();
        let rest = inside
            .get(..MARK.len())
            .filter(|word| word.eq_ignore_ascii_case(MARK))
            .map(|_| &inside[MARK.len()..])?;
        rest.chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && !next.is_whitespace())
            .then(|| &text[open..])
    }
}
