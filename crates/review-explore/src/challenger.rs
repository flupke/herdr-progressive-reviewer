//! What became of the questions the Challenger proposed, so the reviewer can
//! see whether it finds decisions the implementer did not.

use serde::{Deserialize, Serialize};

/// How one of the Challenger's proposals ended on a turn.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalResult {
    /// It is this turn's question.
    Asked,
    /// It was about the same decision as the implementer's question, and the
    /// two became this turn's question.
    Merged,
    /// A fact settles it: one the implementer gave and the source confirms,
    /// or the reviewer's answer.
    Retired,
    /// It waits for a later turn.
    Kept,
}

/// One question the Challenger proposed, and what became of it on a turn.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChallengerProposal {
    /// A short title, the same on every turn that reports the proposal.
    pub title: String,
    pub result: ProposalResult,
    /// Required for `retired`: the fact that settles the proposal, and the
    /// lines that show it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ChallengerProposal {
    /// Whether the proposal has a title and, when retired, a reason. Errors
    /// name the proposal.
    pub(crate) fn validate(&self) -> eyre::Result<()> {
        let title = self.title.trim();
        eyre::ensure!(!title.is_empty(), "Every challenger proposal needs a title");
        eyre::ensure!(
            self.result != ProposalResult::Retired
                || self
                    .reason
                    .as_deref()
                    .is_some_and(|reason| !reason.trim().is_empty()),
            "challenger proposal \"{title}\" is retired and needs `reason`, the fact that \
             settles it and the lines that show it"
        );
        Ok(())
    }
}
