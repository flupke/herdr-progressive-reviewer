use crate::{EvidenceRef, Exploration, InterviewUpdate};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub text: String,
    pub evidence: Vec<EvidenceRef>,
}

/// Immutable agent output, linked to the exact reviewer contribution it responds to.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ConversationTurn {
    pub answer: Option<String>,
    pub update: InterviewUpdate,
}

impl Exploration {
    /// Append-only reference order keeps native viewer identities stable as replies arrive.
    pub fn evidence(&self, turn: usize) -> Vec<EvidenceRef> {
        let Some(question) = self.questions.get(turn) else {
            return Vec::new();
        };
        let mut result = question.evidence.clone();
        result.extend(question.supporting.clone());
        if let Some(assessment) = &question.assessments {
            result.extend(assessment.reversibility.evidence.clone());
            result.extend(assessment.blast_radius.evidence.clone());
        }
        for response in &self.conversation {
            let belongs = (response.update.next.as_ref() == Some(question))
                || response.answer.as_ref().is_some_and(|id| {
                    self.answers.iter().any(|answer| {
                        &answer.id == id && answer.question.as_ref() == Some(question)
                    })
                });
            if belongs && let Some(reply) = &response.update.reply {
                result.extend(reply.evidence.clone());
            }
            if belongs {
                result.extend(
                    response
                        .update
                        .agenda
                        .iter()
                        .flat_map(|change| change.evidence.clone()),
                );
            }
        }
        let mut locations = std::collections::HashSet::new();
        result.retain(|evidence| {
            locations.insert((
                evidence.location.path.clone(),
                evidence.location.side,
                evidence
                    .location
                    .lines
                    .as_ref()
                    .map(|lines| (lines.first_line, lines.last_line)),
            ))
        });
        result
    }
}
