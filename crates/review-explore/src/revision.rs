//! A revised question: the agent's new version of the open question, posted from the round
//! conversation when a message there made the question wrong, without waiting for an answer.

use crate::{ConversationTurn, Exploration, Question};
use review_source::ReviewCheckpoint;
use serde::{Deserialize, Serialize};

/// The agent's new version of the round's open question, as the `revise_question` tool takes
/// it.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionRevision {
    /// Copy Explore round from the latest Explore prompt.
    pub instance: String,
    /// Copy Review unit and Checkpoint from the latest Explore prompt.
    pub checkpoint: ReviewCheckpoint,
    #[serde(flatten)]
    pub revision: Revision,
    /// The open question, in full, with its ID, a higher version and its topic: every field
    /// says what the question asks now.
    pub question: Question,
    /// New terms of the change and changed definitions, as `submit_question` takes them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub glossary: std::collections::BTreeMap<String, String>,
}

/// Why the agent revised the open question.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    /// The ID of the round conversation's message that made the question wrong.
    pub message: String,
    /// What changed in the question and why, in a sentence or two that the reviewer reads above
    /// it.
    pub reason: String,
}

impl Exploration {
    /// Replace the open question with `revision`'s version of it. The new turn keeps what the
    /// turn it follows said and asked to mark, so the reviewer's answer still applies those
    /// marks; whether the revision was new rather than a retry.
    pub fn revise(&mut self, revision: QuestionRevision) -> eyre::Result<bool> {
        eyre::ensure!(
            revision.instance == self.instance,
            "This Explore round is no longer active"
        );
        eyre::ensure!(
            revision.checkpoint == self.comparison.checkpoint,
            "Revision checkpoint does not match"
        );
        let QuestionRevision {
            revision,
            question,
            glossary,
            ..
        } = revision;
        if let Some(previous) = self
            .conversation
            .iter()
            .find(|turn| turn.update.next.as_ref() == Some(&question))
        {
            eyre::ensure!(
                previous.revision.as_ref() == Some(&revision)
                    && previous.update.glossary == glossary,
                "An accepted revision cannot be replaced; retry the identical payload"
            );
            return Ok(false);
        }
        eyre::ensure!(
            self.outstanding.is_none() && self.retry.is_none(),
            "The reviewer's answer is on its way: ask the new version in your next Explore turn"
        );
        let Some((previous, open)) = self
            .conversation
            .last()
            .and_then(|turn| Some((turn.clone(), turn.update.next.clone()?)))
        else {
            eyre::bail!("The round has no open question to revise");
        };
        eyre::ensure!(
            question.id == open.id && question.version > open.version,
            "Revise the open question {}: keep its ID and give a version above {}",
            open.id,
            open.version
        );
        eyre::ensure!(
            !revision.reason.trim().is_empty(),
            "Say in reason what changed in the question and why"
        );
        let mut update = crate::InterviewUpdate {
            request: uuid::Uuid::new_v4().to_string(),
            interpretation: None,
            challenger_proposals: Vec::new(),
            design: None,
            agenda: Vec::new(),
            topics: Vec::new(),
            next: Some(question),
            glossary,
            ..previous.update
        };
        // The marks of the turn it follows wait for the reviewer's answer, as they did there.
        let reviewed = std::mem::take(&mut update.reviewed);
        let reopened = std::mem::take(&mut update.reopened);
        self.validate(&update, &self.comparison)?;
        update.reviewed = reviewed;
        update.reopened = reopened;
        self.conversation.push(ConversationTurn {
            answer: previous.answer,
            update: update.clone(),
            revision: Some(revision),
        });
        self.absorb(update, None);
        Ok(true)
    }
}
