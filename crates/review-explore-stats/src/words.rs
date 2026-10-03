//! How many words of agent text a question puts before the reviewer.
use review_explore::{Alternative, Assessments, Consequence, EvidenceRef, Question, Reply};

/// Agent-written text the reviewer reads, counted in words.
pub(crate) trait Words {
    fn words(&self) -> usize;
}

impl Words for str {
    fn words(&self) -> usize {
        self.split_whitespace().count()
    }
}

impl Words for String {
    fn words(&self) -> usize {
        self.as_str().words()
    }
}

impl<T: Words + ?Sized> Words for &T {
    fn words(&self) -> usize {
        (**self).words()
    }
}

impl<T: Words> Words for Option<T> {
    fn words(&self) -> usize {
        self.as_ref().map_or(0, Words::words)
    }
}

impl<T: Words> Words for [T] {
    fn words(&self) -> usize {
        self.iter().map(Words::words).sum()
    }
}

impl<T: Words> Words for Vec<T> {
    fn words(&self) -> usize {
        self.as_slice().words()
    }
}

impl Words for EvidenceRef {
    fn words(&self) -> usize {
        self.notes.words()
    }
}

impl Words for Alternative {
    fn words(&self) -> usize {
        self.text.words() + self.recommendation.words()
    }
}

impl Words for Consequence {
    fn words(&self) -> usize {
        self.summary.words() + self.details.words() + self.unknowns.words() + self.evidence.words()
    }
}

impl Words for Assessments {
    fn words(&self) -> usize {
        self.reversibility.words() + self.blast_radius.words()
    }
}

impl Words for Reply {
    fn words(&self) -> usize {
        self.text.words() + self.evidence.words()
    }
}

/// The question's own text, context, sketch, choices, evidence notes and
/// assessments; None of the above is the reviewer's, not the agent's.
impl Words for Question {
    fn words(&self) -> usize {
        self.text.words()
            + self.rationale.words()
            + self.visual.words()
            + self.alternatives.words()
            + self.evidence.words()
            + self.assessments.words()
    }
}
