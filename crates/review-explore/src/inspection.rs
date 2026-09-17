use crate::CodeLocation;
use serde::{Deserialize, Serialize};

/// Source investigation kept separate from answered evidence and human decisions.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    /// Changed ranges investigated together as one coherent behavior or interaction.
    pub sources: Vec<CodeLocation>,
    pub behavior: String,
    pub finding: String,
    /// State what remains unknown, or use an empty string when nothing material remains.
    pub uncertainty: String,
    pub disposition: InspectionDisposition,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectionDisposition {
    /// The source raised a useful question that still belongs on the agenda.
    PendingInquiry { topic: String },
    /// The source establishes enough to close this inquiry without asking the reviewer.
    NoFurtherInquiry { reason: String },
    /// A concern remains; attribute a human deferral by its exact Answer ID when applicable.
    Outstanding {
        concern: String,
        #[serde(default)]
        deferred_by: Option<String>,
    },
}

impl InspectionDisposition {
    pub(crate) fn accounts_for_gap(&self) -> bool {
        !matches!(self, Self::PendingInquiry { .. })
    }
}
