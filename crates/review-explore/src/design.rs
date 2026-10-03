//! The design of the change, which the first turn of a round explains before its first question.

use serde::{Deserialize, Serialize};

use crate::{Exploration, QuestionSection};

/// What a reviewer needs to explain the change at a whiteboard without having written it. Each
/// part is Markdown, shown under its own heading in the pane and on the Explore page.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Design {
    /// What the change adds and where: the components it creates or changes, and how they fit
    /// into the code around them.
    pub overview: String,
    /// The main types, the data they hold and store, and how data flows through them.
    pub data_flow: String,
    /// The algorithm and its cost: time, memory, storage, I/O or calls to other systems.
    pub algorithm: String,
    /// The alternatives the implementer rejected, and why, as the description or the code
    /// states them; an inferred alternative says it is inferred.
    pub alternatives: String,
}

impl Design {
    /// The parts, in reading order, under their headings.
    pub fn sections(&self) -> [QuestionSection; 4] {
        [
            ("What it adds and where", &self.overview),
            ("Types and data flow", &self.data_flow),
            ("Algorithm and cost", &self.algorithm),
            ("Rejected alternatives", &self.alternatives),
        ]
        .map(|(title, body)| QuestionSection {
            title,
            body: body.clone(),
        })
    }

    pub(crate) fn validate(&self) -> eyre::Result<()> {
        eyre::ensure!(
            self.sections()
                .iter()
                .all(|section| !section.body.trim().is_empty()),
            "Every part of design needs text; say so when a part does not apply"
        );
        Ok(())
    }
}

impl Exploration {
    /// The design the round's first turn explained, if it did.
    pub fn design(&self) -> Option<&Design> {
        let first = self.conversation.first()?;
        if first.answer.is_some() {
            return None;
        }
        first.update.design.as_ref()
    }
}
