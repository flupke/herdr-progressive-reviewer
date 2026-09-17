use crate::{Exploration, Interpretation, InterviewUpdate};
use review_guide::ReviewCheckpoint;
use serde::{Deserialize, Serialize};

/// Only the agreed implementation scope is editable and sent by the Implement action.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
pub struct Conclusion {
    /// Review outcome, decisions, limitations, and remaining human file inspection.
    pub summary: String,
    /// Only the agreed tasks to implement now, as editable plain text. Empty if none.
    pub to_be_implemented: String,
    /// Deferred or optional work, excluded from the Implement action. Empty if none.
    pub future_work: String,
}

/// A conclusion has its own tool contract; it is not an exploration question.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConclusionSubmission {
    /// Copy the Explore instance from the kickoff or answer wakeup.
    pub review: String,
    pub request: String,
    #[schemars(with = "serde_json::Value")]
    pub checkpoint: ReviewCheckpoint,
    /// Record the exact final answer's decision, using the interview interpretation schema.
    #[schemars(with = "Option<serde_json::Value>")]
    pub interpretation: Option<Interpretation>,
    #[serde(flatten)]
    pub conclusion: Conclusion,
}

impl ConclusionSubmission {
    pub fn into_update(self) -> InterviewUpdate {
        InterviewUpdate {
            instance: self.review,
            request: self.request,
            checkpoint: self.checkpoint,
            interpretation: self.interpretation,
            reply: None,
            agenda: vec![],
            topics: vec![],
            next: None,
            conclusion: Some(self.conclusion),
            limitations: vec![],
            findings: vec![],
        }
    }
}

/// Explicit human authorization, bound to the conclusion that supplied the editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplementationRequest {
    pub instance: String,
    pub conclusion: String,
    pub delivery: String,
    pub text: String,
}

impl Exploration {
    pub fn conclusion_request(&self) -> Option<&str> {
        self.conclusion.as_ref()?;
        self.conversation
            .last()
            .filter(|turn| turn.update.conclusion.is_some())
            .map(|turn| turn.update.request.as_str())
    }

    pub fn implementation(&self, text: String) -> eyre::Result<ImplementationRequest> {
        eyre::ensure!(
            self.outstanding.is_none(),
            "Wait for the pending interview turn"
        );
        eyre::ensure!(
            self.conclusion.is_some(),
            "No current conclusion to implement"
        );
        eyre::ensure!(!text.trim().is_empty(), "Add implementation tasks first");
        let conclusion = self
            .conclusion_request()
            .ok_or_else(|| eyre::eyre!("No accepted conclusion to implement"))?;
        Ok(ImplementationRequest {
            instance: self.instance.clone(),
            conclusion: conclusion.to_owned(),
            delivery: uuid::Uuid::new_v4().to_string(),
            text,
        })
    }
}
