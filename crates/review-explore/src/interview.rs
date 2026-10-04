use crate::{
    AgendaChange, Assessments, CodeLocation, Comparison, ConversationTurn, EvidenceRef, Reply,
};
use review_source::ReviewCheckpoint;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(
    Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    #[default]
    Open,
    Accepted,
    NeedsFollowUp,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
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

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Alternative {
    pub id: String,
    pub text: String,
    pub outcome: TopicStatus,
    pub recommendation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// Stable question ID; use a higher version only to clarify the same question.
    pub id: String,
    #[schemars(range(min = 1))]
    pub version: u32,
    /// ID of an active topic in this round.
    pub topic: String,
    pub text: String,
    /// Context section body: short summary paragraph, then optional Markdown explanation.
    /// Explain relevant behavior and unfamiliar terms without assuming implementation knowledge.
    pub rationale: Option<String>,
    /// Optional simplified/proposed sketch appended to Context; use a fenced Markdown block.
    pub visual: Option<String>,
    /// Distinct choices; the reviewer adds None of the above automatically.
    #[schemars(length(min = 2, max = 5))]
    pub alternatives: Vec<Alternative>,
    /// The lines the question is about, most decisive first, each with notes
    /// explaining what it shows and why it matters to the decision.
    pub evidence: Vec<EvidenceRef>,
    pub assessments: Option<Assessments>,
}

impl Question {
    /// Whether this is version `version` of the question `id`.
    pub fn is_version(&self, id: &str, version: u32) -> bool {
        self.id == id && self.version == version
    }
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
    pub author: String,
    /// The choice, by ID, that the reviewer picked first on a question that hid its
    /// recommendation until then; the agent's prompt leaves it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_pick: Option<String>,
}

impl ReviewerAnswer {
    /// Whether the answer answers `question`, in that version.
    pub(crate) fn answers(&self, question: &Question) -> bool {
        self.question.as_ref() == Some(question)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AnswerInput {
    pub option: Option<String>,
    pub text: String,
    pub in_reply_to: Option<String>,
    /// The choice, by ID, that the reviewer picked before the recommendation showed.
    pub first_pick: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interpretation {
    /// Exact Answer ID from the latest wakeup.
    pub answer: String,
    /// Preserve an unqualified choice's outcome; required changes mean `needs_follow_up`.
    pub status: TopicStatus,
    pub recap: String,
    pub follow_ups: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InterviewUpdate {
    /// Copy Explore round from the latest wakeup.
    pub instance: String,
    /// Copy Explore request from the latest wakeup.
    pub request: String,
    /// Copy Review unit and Checkpoint from the latest wakeup.
    pub checkpoint: ReviewCheckpoint,
    /// Interpret only the latest human decision. Null for kickoff or factual context.
    pub interpretation: Option<Interpretation>,
    /// After a human answer: the changed lines it settled, to mark reviewed. Any
    /// changed lines, cited or not; null lines mark the whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reviewed: Vec<CodeLocation>,
    /// After a human answer: reviewed lines it makes matter again, to reopen. Any
    /// reviewed lines, whoever marked them; null lines reopen the whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reopened: Vec<CodeLocation>,
    /// On any turn: changed lines you read that hold no decision for the
    /// reviewer, to mark reviewed, each with its reason; null lines mark the
    /// whole file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_relevant: Vec<crate::NotRelevantMark>,
    /// In a round with a challenger: what became of each question it
    /// proposed on this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub challenger_proposals: Vec<crate::ChallengerProposal>,
    pub reply: Option<Reply>,
    /// On the first turn, before its question: the design of the change. Null on later turns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<crate::Design>,
    #[serde(default)]
    pub agenda: Vec<AgendaChange>,
    pub topics: Vec<Topic>,
    /// Required for `submit_question`. Use `submit_conclusion` to finish the interview.
    #[schemars(required)]
    pub next: Option<Question>,
    // Stored conclusions share this internal type; the question tool never accepts them.
    #[schemars(skip)]
    pub conclusion: Option<crate::Conclusion>,
    pub limitations: Vec<String>,
    pub findings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct TurnRequest {
    pub instance: String,
    pub request: String,
    pub checkpoint: ReviewCheckpoint,
    pub answer: Option<ReviewerAnswer>,
    /// Answers the reviewer cancelled since the agent's previous request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cancelled: Vec<String>,
    pub response_error: Option<String>,
    /// The round has a challenger, so the prompt carries its script.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub challenger: bool,
}

impl TurnRequest {
    /// Whether the request opens its round: it carries no answer.
    pub fn is_kickoff(&self) -> bool {
        self.answer.is_none()
    }
}

/// The sole decision owner. Agent updates can interpret only the outstanding human answer.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
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
    /// Elapsed time from the reviewer posting a request to its accepted agent response.
    #[serde(default)]
    pub agent_elapsed_ms: BTreeMap<String, u64>,
    pub(crate) outstanding: Option<TurnRequest>,
    pub(crate) retry: Option<TurnRequest>,
    /// Cancelled answers the next request tells the agent about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) cancelled: Vec<String>,
    /// A fresh-context subagent of the agent reviews the change beside it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub challenger: bool,
    /// The diagrams of posted questions that the Explore page could not draw.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagram_errors: Vec<crate::DiagramError>,
    /// What the reviewer did with the quiz of each conclusion, by the request of the agent's
    /// turn that posted it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) quiz_answers: BTreeMap<String, crate::QuizAnswers>,
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
            agent_elapsed_ms: BTreeMap::new(),
            outstanding: None,
            retry: None,
            cancelled: Vec::new(),
            challenger: false,
            diagram_errors: Vec::new(),
            quiz_answers: BTreeMap::new(),
        }
    }

    fn pending(&self) -> bool {
        self.outstanding.is_some()
    }

    /// Whether the reviewer cancelled an answer since the agent's latest turn. No turn came
    /// after the cancellation, so the round asks again what the latest cancelled answer
    /// answered.
    pub fn cancelled_since_last_turn(&self) -> bool {
        !self.cancelled.is_empty()
    }

    /// Whether an agent turn took up the answer `answer`.
    pub fn took_up(&self, answer: &str) -> bool {
        self.turn_after(answer).is_some()
    }

    /// The agent's turn that took up the answer `answer`, if one did.
    pub(crate) fn turn_after(&self, answer: &str) -> Option<&ConversationTurn> {
        self.conversation
            .iter()
            .find(|turn| turn.answer.as_deref() == Some(answer))
    }

    /// How the agent interpreted the answer `answer`, the latest time, if it
    /// did.
    pub fn interpretation(&self, answer: &str) -> Option<&Interpretation> {
        self.interpretations
            .iter()
            .rev()
            .find(|interpretation| interpretation.answer == answer)
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
        // A superseded request may not have told the agent about its cancelled answers.
        let mut cancelled = self
            .retry
            .take()
            .map(|retry| retry.cancelled)
            .unwrap_or_default();
        cancelled.append(&mut self.cancelled);
        let request = TurnRequest {
            instance: self.instance.clone(),
            request: uuid::Uuid::new_v4().to_string(),
            checkpoint: self.comparison.checkpoint.clone(),
            answer,
            cancelled,
            response_error: None,
            challenger: self.challenger,
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
            question.is_some() || input.option.is_none(),
            "A conversation reply has no policy choices"
        );
        eyre::ensure!(
            input.option.is_some() || !input.text.trim().is_empty(),
            "Write an answer or choose an option"
        );
        let option = input
            .option
            .as_ref()
            .map(|id| {
                question
                    .ok_or_else(|| eyre::eyre!("No question is selected"))?
                    .choice(id)
                    .cloned()
                    .ok_or_else(|| eyre::eyre!("Unknown option"))
            })
            .transpose()?;
        if input.option.is_some() {
            eyre::ensure!(
                question.is_some_and(|question| self.can_choose(question)),
                "This choice is no longer pending; reply in free text"
            );
        }
        if let Some(first_pick) = &input.first_pick {
            eyre::ensure!(
                question.is_some_and(|question| question.choice(first_pick).is_some()),
                "The first pick is not a choice of this question"
            );
        }
        let answer = ReviewerAnswer {
            id: uuid::Uuid::new_v4().to_string(),
            checkpoint: self.comparison.checkpoint.clone(),
            question: question.cloned(),
            in_reply_to,
            option,
            text: input.text,
            author: "reviewer".into(),
            first_pick: input.first_pick,
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
        let request = self
            .retry
            .clone()
            .ok_or_else(|| eyre::eyre!("No request to retry"))?;
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
        let answer = request.answer.clone();
        self.conversation.push(ConversationTurn {
            answer: answer.as_ref().map(|answer| answer.id.clone()),
            update: update.clone(),
        });
        self.absorb(update, answer.as_ref());
        self.outstanding = None;
        self.retry = None;
        Ok(true)
    }

    /// Take an accepted agent turn, following `answer`, into the interview's
    /// topics, questions, decisions and report.
    pub(crate) fn absorb(&mut self, update: InterviewUpdate, answer: Option<&ReviewerAnswer>) {
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
            "This Explore round is no longer active"
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
