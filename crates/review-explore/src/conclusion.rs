use crate::{Exploration, Interpretation, InterviewUpdate};
use review_source::ReviewCheckpoint;
use serde::{Deserialize, Serialize};

/// Only the agreed implementation scope is editable and sent by the Implement action.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
pub struct Conclusion {
    /// Review outcome, limitations and remaining uncertainty. The reviewer's decisions are listed
    /// beside it from the round itself, so it does not restate them.
    pub summary: String,
    /// Only the agreed tasks to implement now, as editable plain text. Empty if none.
    pub to_be_implemented: String,
    /// Optional or later work, excluded from the Implement action. Empty if none.
    pub future_work: String,
    /// Questions at whiteboard level, one for each key aspect of the change. Where the reviewer
    /// follows the round on the Explore page, the page asks them before the summary; the pane
    /// does not ask them. Empty when the change has nothing at that level. Rounds saved before
    /// quizzes existed have none, so the schema cannot require it: the tool refuses a
    /// conclusion with neither items nor `quiz_empty_reason`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quiz: Vec<crate::QuizItem>,
    /// Required when the quiz is empty: why, in one sentence. Null when it has items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiz_empty_reason: Option<String>,
}

/// A conclusion has its own tool contract; it is not an exploration question.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConclusionSubmission {
    /// Copy the renewable Explore access from the latest kickoff or answer wakeup.
    pub review: String,
    /// Durable Explore round identity, independent of renewable review access.
    pub instance: String,
    /// Copy Explore request from the latest wakeup.
    pub request: String,
    /// Copy Review unit and Checkpoint from the latest wakeup.
    pub checkpoint: ReviewCheckpoint,
    /// Record the latest human decision using its exact Answer ID; otherwise null.
    pub interpretation: Option<Interpretation>,
    /// After a human answer: the changed lines it settled, to mark reviewed. Any
    /// changed lines, cited or not; null lines mark the whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewed: Vec<crate::CodeLocation>,
    /// After a human answer: reviewed lines it makes matter again, to reopen. Any
    /// reviewed lines, whoever marked them; null lines reopen the whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reopened: Vec<crate::CodeLocation>,
    /// Changed lines you read that hold no decision for the reviewer, to mark
    /// reviewed at once, each with its reason; null lines mark the whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_relevant: Vec<crate::NotRelevantMark>,
    /// In a round with a challenger: what became of each question it
    /// proposed on this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub challenger_proposals: Vec<crate::ChallengerProposal>,
    #[serde(flatten)]
    pub conclusion: Conclusion,
}

impl ConclusionSubmission {
    pub fn into_update(self) -> InterviewUpdate {
        InterviewUpdate {
            instance: self.instance,
            request: self.request,
            checkpoint: self.checkpoint,
            interpretation: self.interpretation,
            reviewed: self.reviewed,
            reopened: self.reopened,
            not_relevant: self.not_relevant,
            challenger_proposals: self.challenger_proposals,
            reply: None,
            design: None,
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
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
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
