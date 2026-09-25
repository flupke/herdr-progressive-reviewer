//! Durable investigation state. Sources and runtime capabilities are never stored here.
use crate::{
    CoverageLedger, CoverageReceipt, Exploration, ImplementationRequest, InterviewUpdate,
    TurnRequest,
};
use herdr_client::protocol::AgentSession;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExploreHistory {
    pub passes: Vec<String>,
    #[serde(default = "latest_editable")]
    pub latest_editable: bool,
}

fn latest_editable() -> bool {
    true
}

impl Default for ExploreHistory {
    fn default() -> Self {
        Self {
            passes: Vec::new(),
            latest_editable: true,
        }
    }
}

impl ExploreHistory {
    pub fn is_historical(&self, instance: &str) -> bool {
        !self.latest_editable || self.passes.last().map(String::as_str) != Some(instance)
    }
}

/// Native identity alone permits resumption in a restarted pane.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ConversationBinding {
    agent: Option<String>,
    session: AgentSession,
}

impl ConversationBinding {
    pub fn from_agent(agent: &herdr_client::protocol::Agent) -> Option<Self> {
        Some(Self {
            agent: agent.agent.clone(),
            session: agent.agent_session.clone()?,
        })
    }

    pub fn matches(&self, agent: &herdr_client::protocol::Agent) -> bool {
        self.agent == agent.agent
            && agent.agent_session.as_ref().is_some_and(|session| {
                self.session.agent == session.agent
                    && self.session.kind == session.kind
                    && self.session.value == session.value
            })
    }

    pub fn same_agent_kind(&self, agent: &herdr_client::protocol::Agent) -> bool {
        self.agent == agent.agent
            && agent.agent_session.as_ref().is_some_and(|session| {
                self.session.agent == session.agent && self.session.kind == session.kind
            })
    }
}

/// What the dispatcher can prove, independently of UI lifecycle or task completion.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub enum DispatchState {
    #[default]
    Queued,
    /// Written immediately before the external call; recovery must assume it may have sent.
    Attempting,
    Delivered,
    NotSent(String),
    Cancelled,
    Unknown,
}

impl DispatchState {
    #[must_use]
    pub fn recovered(&self) -> Self {
        if *self == Self::Attempting {
            Self::Unknown
        } else {
            self.clone()
        }
    }

    fn may_retry(&self) -> bool {
        matches!(self, Self::Queued | Self::NotSent(_) | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ImplementationDelivery {
    pub authorized_at: u64,
    pub attempt: String,
    pub request: ImplementationRequest,
    binding: ConversationBinding,
    pub state: DispatchState,
}

/// Identifies a particular queued attempt without changing its logical request.
#[derive(Clone, Debug)]
pub enum DispatchId {
    Interview { request: String, attempt: String },
    Implementation { request: String, attempt: String },
}

pub struct DispatchResult {
    pub id: DispatchId,
    pub began: bool,
    pub state: DispatchState,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct InterviewDelivery {
    pub attempt: String,
    pub editor_sequence: Option<u64>,
    pub request: TurnRequest,
    pub state: DispatchState,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct PriorMark {
    pub baseline: String,
    pub reviewed_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CompletionMark {
    pub path: Vec<u8>,
    pub prior: Option<PriorMark>,
    pub applied: bool,
}

/// A saved local transaction; completion is acknowledged only after every path is marked.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ReviewCompletion {
    pub request: String,
    pub baseline: String,
    pub marks: Vec<CompletionMark>,
    pub completed: bool,
    #[serde(default)]
    pub exclusions_enabled: bool,
    #[serde(default)]
    pub summary: crate::CoverageSummary,
    /// Unanswered checkpoint geometry captured before file marks are applied.
    #[serde(default)]
    pub unexplored: Option<UnexploredAtConclusion>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct UnexploredAtConclusion {
    pub required: Vec<crate::CoverageUnit>,
    pub jev_excluded: Vec<crate::CoverageUnit>,
}

/// Domain and deduplication state are committed together, separate from editor autosaves.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExplorePass {
    pub revision: u64,
    pub exploration: Exploration,
    #[serde(default)]
    pub coverage: CoverageLedger,
    pub binding: Option<ConversationBinding>,
    pub turns: BTreeMap<String, InterviewDelivery>,
    pub implementations: BTreeMap<String, ImplementationDelivery>,
    #[serde(default)]
    pub completion: Option<ReviewCompletion>,
    #[serde(default)]
    pub coverage_receipts: BTreeMap<String, CoverageReceipt>,
}

impl ExplorePass {
    pub fn new(exploration: Exploration) -> Self {
        let coverage = CoverageLedger::new(&exploration.comparison);
        Self {
            revision: 0,
            exploration,
            coverage,
            binding: None,
            turns: BTreeMap::new(),
            implementations: BTreeMap::new(),
            completion: None,
            coverage_receipts: BTreeMap::new(),
        }
    }

    /// Validate on a candidate so rejection retains the pending request and answer.
    pub fn submit(
        &mut self,
        update: &InterviewUpdate,
        exclusions_enabled: bool,
    ) -> eyre::Result<(bool, CoverageReceipt)> {
        let mut candidate = self.exploration.clone();
        let applied = candidate.submit(update.clone())?;
        if !applied && let Some(receipt) = self.coverage_receipts.get(&update.request) {
            return Ok((false, receipt.clone()));
        }
        if update.conclusion.is_some() && applied {
            eyre::ensure!(
                self.coverage.inventory.complete,
                "coverage_incomplete: {}",
                self.coverage.inventory.limitations.join("; ")
            );
        }
        if applied {
            if let Some(started) = self
                .turns
                .get(&update.request)
                .and_then(|turn| turn.started_at_ms)
            {
                let finished = now_ms();
                candidate
                    .agent_elapsed_ms
                    .insert(update.request.clone(), finished.saturating_sub(started));
            }
            self.exploration = candidate;
            if update.next.is_some() {
                self.coverage.revision += 1;
            }
        }
        let pending =
            self.exploration
                .questions
                .last()
                .filter(|question| {
                    !self.exploration.answers.iter().any(|answer| {
                        answer.question.as_ref() == Some(question) && !answer.deferred
                    })
                })
                .cloned()
                .into_iter()
                .collect::<Vec<_>>();
        let feedback = if let Some(question) = &update.next {
            CoverageReceipt::AfterAnswer {
                coverage_after_answer: self.coverage.feedback_after_answer(
                    &self.exploration.comparison,
                    question,
                    exclusions_enabled,
                ),
            }
        } else {
            CoverageReceipt::Current(self.coverage.feedback(
                &self.exploration.comparison,
                &pending,
                exclusions_enabled,
            ))
        };
        if applied {
            self.coverage_receipts
                .insert(update.request.clone(), feedback.clone());
        }
        Ok((applied, feedback))
    }

    /// Accept an already attributed UI contribution against the latest stored state.
    pub fn post(&mut self, request: &TurnRequest) -> eyre::Result<bool> {
        eyre::ensure!(
            self.completion
                .as_ref()
                .is_none_or(|completion| completion.completed),
            "Explore conclusion finalization is pending; recover file marking before posting"
        );
        eyre::ensure!(
            request.instance == self.exploration.instance
                && request.checkpoint == self.exploration.comparison.checkpoint,
            "This answer belongs to another Explore pass"
        );
        if let Some(previous) = self.turns.get_mut(&request.request) {
            eyre::ensure!(
                self.exploration
                    .retry
                    .as_ref()
                    .is_some_and(|current| current.request == request.request)
                    && self
                        .exploration
                        .outstanding
                        .as_ref()
                        .is_none_or(|current| current.request == request.request),
                "This interrupted turn has been superseded; restore the latest question"
            );
            eyre::ensure!(
                previous.request.answer == request.answer
                    && previous.request.checkpoint == request.checkpoint,
                "This turn identity already has different content"
            );
            eyre::ensure!(
                !self
                    .exploration
                    .conversation
                    .iter()
                    .any(|turn| turn.update.request == request.request),
                "The response is already saved; restore it instead of sending again"
            );
            previous.attempt = uuid::Uuid::new_v4().to_string();
            previous.state = DispatchState::Queued;
            previous.request = request.clone();
            previous.started_at_ms = None;
            self.exploration.outstanding = Some(request.clone());
            self.exploration.retry = Some(request.clone());
            return Ok(false);
        }
        let mut candidate = self.exploration.clone();
        let input = request.answer.as_ref().map(|answer| crate::AnswerInput {
            option: answer.option.as_ref().map(|option| option.id.clone()),
            text: answer.text.clone(),
            deferred: answer.deferred,
            corrects: answer.corrects.clone(),
            in_reply_to: Some(answer.in_reply_to.clone()),
        });
        let validated = candidate.request(
            input,
            request
                .answer
                .as_ref()
                .and_then(|answer| answer.question.as_ref()),
        )?;
        if let (Some(mut expected), Some(answer)) = (validated.answer, &request.answer) {
            expected.id.clone_from(&answer.id);
            eyre::ensure!(
                expected == *answer,
                "Answer content does not match the displayed question"
            );
            eyre::ensure!(
                !self
                    .exploration
                    .answers
                    .iter()
                    .any(|old| old.id == answer.id),
                "Answer identity already used"
            );
            self.exploration.answers.push(answer.clone());
            self.coverage.credit(answer, &self.exploration.comparison);
        }
        self.exploration.outstanding = Some(request.clone());
        self.exploration.retry = Some(request.clone());
        self.turns.insert(
            request.request.clone(),
            InterviewDelivery {
                editor_sequence: None,
                attempt: uuid::Uuid::new_v4().to_string(),
                request: request.clone(),
                state: DispatchState::Queued,
                started_at_ms: None,
            },
        );
        Ok(true)
    }

    pub fn authorize(&mut self, request: &ImplementationRequest) -> eyre::Result<bool> {
        eyre::ensure!(
            self.completion
                .as_ref()
                .is_some_and(|completion| completion.completed),
            "Explore file marking is not complete"
        );
        eyre::ensure!(
            request.instance == self.exploration.instance
                && self.exploration.conclusion_request() == Some(request.conclusion.as_str()),
            "This conclusion is no longer current"
        );
        self.exploration.implementation(request.text.clone())?;
        if let Some(previous) = self.implementations.get_mut(&request.delivery) {
            eyre::ensure!(
                previous.request == *request,
                "Implementation authorization cannot change"
            );
            eyre::ensure!(
                previous.state.may_retry(),
                "Implementation delivery is already sent or uncertain"
            );
            previous.attempt = uuid::Uuid::new_v4().to_string();
            previous.state = DispatchState::Queued;
            return Ok(false);
        }
        let binding = self
            .binding
            .clone()
            .ok_or_else(|| eyre::eyre!("Original agent conversation is not yet identified"))?;
        self.implementations.insert(
            request.delivery.clone(),
            ImplementationDelivery {
                authorized_at: self.revision + 1,
                attempt: uuid::Uuid::new_v4().to_string(),
                request: request.clone(),
                binding,
                state: DispatchState::Queued,
            },
        );
        Ok(true)
    }

    fn dispatch_state(&mut self, id: &DispatchId) -> Option<&mut DispatchState> {
        match id {
            DispatchId::Interview { request, attempt } => self
                .turns
                .get_mut(request)
                .filter(|record| &record.attempt == attempt)
                .map(|record| &mut record.state),
            DispatchId::Implementation { request, attempt } => self
                .implementations
                .get_mut(request)
                .filter(|record| &record.attempt == attempt)
                .map(|record| &mut record.state),
        }
    }

    pub fn begin_dispatch(
        &mut self,
        id: &DispatchId,
        agent: &herdr_client::protocol::Agent,
    ) -> eyre::Result<()> {
        if let DispatchId::Interview { request, .. } = id {
            eyre::ensure!(
                self.exploration
                    .outstanding
                    .as_ref()
                    .is_some_and(|current| &current.request == request),
                "Interview turn cancelled or superseded"
            );
        }
        if let Some(binding) = &self.binding {
            eyre::ensure!(
                binding.matches(agent),
                "Different agent conversation; return to the original or start a New pass"
            );
        } else {
            self.binding = ConversationBinding::from_agent(agent);
            eyre::ensure!(
                matches!(id, DispatchId::Interview { .. }) || self.binding.is_some(),
                "Native agent identity is not yet reported"
            );
        }
        let state = self
            .dispatch_state(id)
            .ok_or_else(|| eyre::eyre!("Delivery attempt superseded"))?;
        eyre::ensure!(
            *state == DispatchState::Queued,
            "Delivery already attempted or cancelled"
        );
        *state = DispatchState::Attempting;
        if let DispatchId::Interview { request, .. } = id
            && let Some(turn) = self.turns.get_mut(request)
        {
            turn.started_at_ms = Some(now_ms());
        }
        Ok(())
    }

    /// Superseded callbacks and duplicate observers cannot overwrite newer dispatch knowledge.
    pub fn finish_dispatch(&mut self, result: &DispatchResult) {
        if let Some(state) = self.dispatch_state(&result.id)
            && ((result.began && *state == DispatchState::Attempting)
                || (!result.began && *state == DispatchState::Queued))
        {
            *state = result.state.clone();
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}
