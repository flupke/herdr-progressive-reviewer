use crate::{Comparison, EvidenceRef};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Door {
    OneWay,
    TwoWay,
    Mixed,
    Unknown,
}

impl Door {
    pub fn label(self) -> &'static str {
        match self {
            Self::OneWay => "One-way / hard to reverse",
            Self::TwoWay => "Two-way",
            Self::Mixed => "Mixed",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Consequence {
    /// Short first paragraph for this Markdown section, including the decisive reason.
    pub summary: String,
    /// Optional Markdown reasoning after the summary; omit or leave empty when unnecessary.
    #[serde(default)]
    pub details: String,
    pub evidence: Vec<EvidenceRef>,
    pub unknowns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assessments {
    pub door: Door,
    pub reversibility: Consequence,
    pub blast_radius: Consequence,
}

impl Assessments {
    pub(crate) fn validate(&self, comparison: &Comparison) -> eyre::Result<()> {
        for lens in [&self.reversibility, &self.blast_radius] {
            eyre::ensure!(
                !lens.summary.trim().is_empty()
                    && (!lens.evidence.is_empty() || !lens.unknowns.is_empty())
                    && lens
                        .unknowns
                        .iter()
                        .all(|unknown| !unknown.trim().is_empty())
                    && lens
                        .evidence
                        .iter()
                        .all(|evidence| comparison.validate_evidence(evidence)),
                "Assessments need a reason and valid evidence, or explicit unknowns"
            );
        }
        eyre::ensure!(
            !matches!(self.door, Door::OneWay | Door::TwoWay)
                || !self.reversibility.evidence.is_empty(),
            "A known door assessment needs evidence; otherwise use unknown"
        );
        eyre::ensure!(
            self.door != Door::Unknown || !self.reversibility.unknowns.is_empty(),
            "An unknown door must name the missing evidence"
        );
        Ok(())
    }
}
