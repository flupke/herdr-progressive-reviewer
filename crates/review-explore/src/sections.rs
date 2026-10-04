//! The sections a question shows under their own headings, in the pane and on the Explore page.

use serde::Serialize;

use crate::{Assessments, Consequence, Question};

/// A section of a question: its heading, its lead (the decisive reason, which the Explore page
/// shows while the section is folded), and the rest of its body, in Markdown.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QuestionSection {
    pub title: &'static str,
    /// The section's first paragraph.
    pub lead: String,
    /// The paragraphs after the lead; empty when it has none.
    pub details: String,
}

impl QuestionSection {
    /// The whole body: the lead, then the details.
    pub fn body(&self) -> String {
        if self.details.is_empty() {
            return self.lead.clone();
        }
        format!("{}\n\n{}", self.lead, self.details)
    }
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
                lead: format!("{} — {}", self.door.label(), self.reversibility.summary),
                details: Self::details(&self.reversibility),
            },
            QuestionSection {
                title: "Blast radius",
                lead: self.blast_radius.summary.clone(),
                details: Self::details(&self.blast_radius),
            },
        ]
    }

    /// The lens's details, then each of its unknowns.
    fn details(lens: &Consequence) -> String {
        let unknowns = lens
            .unknowns
            .iter()
            .map(|unknown| format!("Unknown: {unknown}"));
        (!lens.details.trim().is_empty())
            .then(|| lens.details.clone())
            .into_iter()
            .chain(unknowns)
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
