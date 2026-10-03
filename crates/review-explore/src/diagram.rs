//! Diagrams of a question's explanation that the Explore page could not draw.

use serde::{Deserialize, Serialize};

use crate::Exploration;

/// A diagram in a question's Markdown that Mermaid could not parse on the Explore page. The
/// round keeps it with the question; the tool cannot check a diagram when the agent submits it.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct DiagramError {
    /// The question's ID.
    pub question: String,
    /// The question's version.
    pub version: u32,
    /// The diagram's Mermaid source, as its fenced block holds it.
    pub source: String,
    /// Mermaid's message.
    pub message: String,
}

impl DiagramError {
    fn same_diagram(&self, other: &Self) -> bool {
        self.question == other.question
            && self.version == other.version
            && self.source == other.source
    }
}

impl Exploration {
    /// Keeps `error` with its question, replacing an earlier error of the same diagram. Returns
    /// false when the round already holds this error. Refuses an error for a question the round
    /// never posted.
    pub fn record_diagram_error(&mut self, error: DiagramError) -> eyre::Result<bool> {
        eyre::ensure!(
            self.posted(&error),
            "the round never posted version {} of question {}",
            error.version,
            error.question
        );
        match self
            .diagram_errors
            .iter_mut()
            .find(|kept| kept.same_diagram(&error))
        {
            Some(kept) if *kept == error => Ok(false),
            Some(kept) => {
                *kept = error;
                Ok(true)
            }
            None => {
                self.diagram_errors.push(error);
                Ok(true)
            }
        }
    }

    /// Drops the errors of questions the round no longer holds, after a turn was removed.
    pub(crate) fn forget_diagram_errors_of_withdrawn_questions(&mut self) {
        let mut errors = std::mem::take(&mut self.diagram_errors);
        errors.retain(|error| self.posted(error));
        self.diagram_errors = errors;
    }

    /// Whether the round holds the question of `error`.
    fn posted(&self, error: &DiagramError) -> bool {
        self.questions
            .iter()
            .any(|question| question.is_version(&error.question, error.version))
    }
}
