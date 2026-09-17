use crate::{
    AgendaChange, Assessments, CodeLocation, Comparison, ConversationTurn, EvidenceRef, Reply,
};
use review_guide::ReviewCheckpoint;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    #[default]
    Open,
    Accepted,
    NeedsFollowUp,
    Deferred,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Topic {
    pub id: String,
    pub title: String,
    pub entries: Vec<CodeLocation>,
    #[serde(default)]
    pub status: TopicStatus,
    /// Provisional inquiry; posted questions retain their own immutable wording.
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub rank: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub id: String,
    pub text: String,
    pub outcome: TopicStatus,
    pub recommendation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub version: u32,
    pub topic: String,
    pub text: String,
    pub rationale: Option<String>,
    pub visual: Option<String>,
    pub alternatives: Vec<Alternative>,
    pub evidence: Vec<EvidenceRef>,
    /// Additional context, opened on demand rather than promoted into the question.
    #[serde(default)]
    pub supporting: Vec<EvidenceRef>,
    pub assessments: Option<Assessments>,
}

/// A literal reviewer contribution; choosing an option preserves its stable ID and wording.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ReviewerAnswer {
    pub id: String,
    pub checkpoint: ReviewCheckpoint,
    pub question: Option<Question>,
    pub in_reply_to: String,
    pub option: Option<Alternative>,
    pub text: String,
    pub deferred: bool,
    pub corrects: Option<String>,
    pub author: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnswerInput {
    pub option: Option<String>,
    pub text: String,
    pub deferred: bool,
    pub corrects: Option<String>,
    pub in_reply_to: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Interpretation {
    pub answer: String,
    pub status: TopicStatus,
    pub recap: String,
    pub follow_ups: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InterviewUpdate {
    pub instance: String,
    pub request: String,
    pub checkpoint: ReviewCheckpoint,
    pub interpretation: Option<Interpretation>,
    pub reply: Option<Reply>,
    #[serde(default)]
    pub agenda: Vec<AgendaChange>,
    pub topics: Vec<Topic>,
    pub next: Option<Question>,
    pub conclusion: Option<crate::Conclusion>,
    pub limitations: Vec<String>,
    pub findings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
pub struct TurnRequest {
    pub instance: String,
    pub request: String,
    pub checkpoint: ReviewCheckpoint,
    pub answer: Option<ReviewerAnswer>,
    pub response_error: Option<String>,
}

/// The sole decision owner. Agent updates can interpret only the outstanding human answer.
#[derive(Clone, Debug)]
pub struct Exploration {
    pub instance: String,
    pub comparison: Arc<Comparison>,
    pub topics: BTreeMap<String, Topic>,
    pub questions: Vec<Question>,
    pub answers: Vec<ReviewerAnswer>,
    pub interpretations: Vec<Interpretation>,
    pub limitations: Vec<String>,
    pub findings: Vec<String>,
    pub conclusion: Option<crate::Conclusion>,
    pub conversation: Vec<ConversationTurn>,
    pub(crate) outstanding: Option<TurnRequest>,
    pub(crate) retry: Option<TurnRequest>,
}

impl Exploration {
    pub fn new(comparison: Arc<Comparison>) -> Self {
        Self {
            instance: uuid::Uuid::new_v4().to_string(),
            comparison,
            topics: BTreeMap::new(),
            questions: Vec::new(),
            answers: Vec::new(),
            interpretations: Vec::new(),
            limitations: Vec::new(),
            findings: Vec::new(),
            conclusion: None,
            conversation: Vec::new(),
            outstanding: None,
            retry: None,
        }
    }

    fn pending(&self) -> bool {
        self.outstanding.is_some()
    }

    pub fn request(
        &mut self,
        input: Option<AnswerInput>,
        question: Option<&Question>,
    ) -> eyre::Result<TurnRequest> {
        eyre::ensure!(!self.pending(), "An interview request is already pending");
        let answer = input
            .map(|input| self.record(input, question))
            .transpose()?;
        let request = TurnRequest {
            instance: self.instance.clone(),
            request: uuid::Uuid::new_v4().to_string(),
            checkpoint: self.comparison.checkpoint.clone(),
            answer,
            response_error: None,
        };
        self.outstanding = Some(request.clone());
        self.retry = Some(request.clone());
        Ok(request)
    }

    fn record(
        &mut self,
        input: AnswerInput,
        question: Option<&Question>,
    ) -> eyre::Result<ReviewerAnswer> {
        let context = self
            .conversation
            .iter()
            .rev()
            .find(|turn| match question {
                Some(question) => turn.update.next.as_ref() == Some(question),
                None => {
                    turn.update.conclusion.is_some()
                        && input
                            .in_reply_to
                            .as_ref()
                            .is_none_or(|id| id == &turn.update.request)
                }
            })
            .ok_or_else(|| eyre::eyre!("Unknown question or conversation context"))?;
        let in_reply_to = context.update.request.clone();
        eyre::ensure!(
            question.is_some() || (input.option.is_none() && !input.deferred),
            "A conversation reply has no policy choices or deferral"
        );
        eyre::ensure!(
            input.deferred || input.option.is_some() || !input.text.trim().is_empty(),
            "Write an answer or choose an option"
        );
        let option = input
            .option
            .as_ref()
            .map(|id| {
                question
                    .ok_or_else(|| eyre::eyre!("No question is selected"))?
                    .choices()
                    .find(|option| &option.id == id)
                    .cloned()
                    .ok_or_else(|| eyre::eyre!("Unknown option"))
            })
            .transpose()?;
        if let Some(corrects) = &input.corrects {
            eyre::ensure!(
                self.answers.iter().any(|answer| &answer.id == corrects
                    && answer.question.as_ref() == question
                    && answer.in_reply_to == in_reply_to),
                "Correction must refer to this exact question"
            );
        } else if input.option.is_some() {
            eyre::ensure!(
                question.is_some_and(|question| self.can_choose(question)),
                "This choice is no longer pending; reply in free text or use Correct"
            );
        }
        let answer = ReviewerAnswer {
            id: uuid::Uuid::new_v4().to_string(),
            checkpoint: self.comparison.checkpoint.clone(),
            question: question.cloned(),
            in_reply_to,
            option,
            text: input.text,
            deferred: input.deferred,
            corrects: input.corrects,
            author: "reviewer".into(),
        };
        self.answers.push(answer.clone());
        Ok(answer)
    }

    pub fn cancel(&mut self) {
        self.outstanding = None;
    }

    pub fn failed(&mut self, request: &str, error: &str) -> bool {
        if self
            .outstanding
            .as_ref()
            .is_none_or(|turn| turn.request != request)
        {
            return false;
        }
        self.outstanding = None;
        if let Some(retry) = &mut self.retry {
            retry.response_error = Some(error.to_owned());
        }
        true
    }

    pub fn retry(&mut self) -> eyre::Result<TurnRequest> {
        eyre::ensure!(!self.pending(), "An interview request is already pending");
        let mut request = self
            .retry
            .clone()
            .ok_or_else(|| eyre::eyre!("No request to retry"))?;
        request.request = uuid::Uuid::new_v4().to_string();
        self.retry = Some(request.clone());
        self.outstanding = Some(request.clone());
        Ok(request)
    }

    pub fn apply(&mut self, update: InterviewUpdate) -> eyre::Result<bool> {
        let Some(request) = &self.outstanding else {
            return Ok(false);
        };
        if request.instance != update.instance || request.request != update.request {
            return Ok(false);
        }
        self.validate(&update, &self.comparison)?;
        let answer = request.answer.as_ref();
        self.conversation.push(ConversationTurn {
            answer: answer.map(|answer| answer.id.clone()),
            update: update.clone(),
        });
        for topic in update.topics {
            self.topics.insert(topic.id.clone(), topic);
        }
        if let Some((interpretation, answer)) = update.interpretation.zip(answer) {
            if let Some(question) = &answer.question
                && let Some(topic) = self.topics.get_mut(&question.topic)
            {
                topic.status = interpretation.status;
            }
            self.interpretations.push(interpretation);
        }
        if let Some(question) = update.next {
            self.questions.push(question);
        }
        self.record_report(
            update.limitations,
            update.findings,
            update.conclusion.is_some(),
        );
        self.conclusion = update.conclusion;
        self.outstanding = None;
        self.retry = None;
        Ok(true)
    }

    fn record_report(&mut self, limitations: Vec<String>, findings: Vec<String>, concluded: bool) {
        if !concluded || !limitations.is_empty() {
            self.limitations = limitations;
        }
        for finding in findings {
            if !self.findings.contains(&finding) {
                self.findings.push(finding);
            }
        }
    }

    /// MCP retries acknowledge an identical accepted turn without replaying it.
    pub fn submit(&mut self, update: InterviewUpdate) -> eyre::Result<bool> {
        eyre::ensure!(
            update.instance == self.instance,
            "This Explore pass is no longer active"
        );
        if let Some(previous) = self
            .conversation
            .iter()
            .find(|turn| turn.update.request == update.request)
        {
            eyre::ensure!(
                previous.update == update,
                "An accepted turn cannot be replaced; retry the identical payload"
            );
            return Ok(false);
        }
        eyre::ensure!(
            self.outstanding
                .as_ref()
                .is_some_and(|request| request.request == update.request),
            "This Explore request is cancelled or obsolete"
        );
        self.apply(update)
    }

    pub fn unmapped(&self) -> Vec<&crate::ManifestEntry> {
        self.comparison
            .manifest
            .iter()
            .filter(|entry| {
                !self.topics.values().any(|topic| {
                    topic
                        .entries
                        .iter()
                        .any(|location| self.comparison.maps(entry, location))
                })
            })
            .collect()
    }
}
