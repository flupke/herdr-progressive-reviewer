//! The sections a question shows under their own headings, in the pane and on the Explore page.

use serde::Serialize;

use crate::{Assessments, Consequence, Question};

/// A section of a question: its heading, and its body in Markdown.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QuestionSection {
    pub title: &'static str,
    pub body: String,
}

impl Question {
    /// The Context section's body: the rationale, then the visual.
    pub fn context(&self) -> String {
        [&self.rationale, &self.visual]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl Assessments {
    /// The Door and Blast radius sections, in that order.
    pub fn sections(&self) -> [QuestionSection; 2] {
        [
            QuestionSection {
                title: "Door",
                body: Self::body(
                    format!("{} — {}", self.door.label(), self.reversibility.summary),
                    &self.reversibility,
                ),
            },
            QuestionSection {
                title: "Blast radius",
                body: Self::body(self.blast_radius.summary.clone(), &self.blast_radius),
            },
        ]
    }

    /// `summary`, then the lens's details and each of its unknowns.
    fn body(mut summary: String, lens: &Consequence) -> String {
        if !lens.details.trim().is_empty() {
            summary.push_str("\n\n");
            summary.push_str(&lens.details);
        }
        for unknown in &lens.unknowns {
            summary.push_str("\n\nUnknown: ");
            summary.push_str(unknown);
        }
        summary
    }
}
