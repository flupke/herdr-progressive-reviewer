//! Durable investigation state. Sources and runtime capabilities are never stored here.
use crate::{Exploration, ImplementationRequest, InterviewUpdate, TurnRequest};
use herdr_client::protocol::AgentSession;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExploreHistory {
    /// Indexes saved before the rename call this field `passes`.
    #[serde(alias = "passes")]
    pub rounds: Vec<String>,
    #[serde(default = "latest_editable")]
    pub latest_editable: bool,
}

fn latest_editable() -> bool {
    true
}

impl Default for ExploreHistory {
    fn default() -> Self {
        Self {
            rounds: Vec::new(),
            latest_editable: true,
        }
    }
}

impl ExploreHistory {
    pub fn is_historical(&self, instance: &str) -> bool {
        !self.latest_editable || self.rounds.last().map(String::as_str) != Some(instance)
    }
}

/// A native conversation identity observed while handling this round.
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

/// The first conclusion the round accepted, which authorizes implementation.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ReviewCompletion {
    pub request: String,
    pub baseline: String,
}

/// Domain and deduplication state are committed together, separate from editor autosaves.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExploreRound {
    pub revision: u64,
    pub exploration: Exploration,
    #[serde(default)]
    pub last_agent_session: Option<ConversationBinding>,
    pub turns: BTreeMap<String, InterviewDelivery>,
    pub implementations: BTreeMap<String, ImplementationDelivery>,
    #[serde(default)]
    pub completion: Option<ReviewCompletion>,
    /// The review marks each agent turn changed, by Explore request.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marks: BTreeMap<String, TurnMarks>,
}

/// The review marks one agent turn changed.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TurnMarks {
    /// The answer that applied the marks: the one a question turn got, or the
    /// one a conclusion follows. A conclusion that opens the round has none.
    pub answer: Option<String>,
    /// The lines an answer settled, marked reviewed, as they were applied.
    pub reviewed: Vec<crate::CodeLocation>,
    /// The lines the agent read and found to hold no decision, marked
    /// reviewed, as they were applied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_relevant: Vec<crate::CodeLocation>,
    /// The lines reopened, with who had marked them.
    pub reopened: Vec<ReopenedLines>,
    /// Why some or all of the requested marks were not applied.
    #[serde(default)]
    pub problem: Option<String>,
}

/// Reviewed lines a turn reopened, and who had marked them.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ReopenedLines {
    #[serde(flatten)]
    pub location: crate::CodeLocation,
    pub author: review_types::MarkAuthor,
}

/// How many lines and whole files a turn marked reviewed and reopened.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkCounts {
    pub reviewed_lines: u32,
    pub reviewed_files: u32,
    pub not_relevant_lines: u32,
    pub not_relevant_files: u32,
    pub reopened_lines: u32,
    pub reopened_files: u32,
}

impl InterviewUpdate {
    /// Whether the turn asks for review marks.
    pub fn requests_marks(&self) -> bool {
        !(self.reviewed.is_empty() && self.reopened.is_empty() && self.not_relevant.is_empty())
    }

    /// How much the turn asks to mark, before any of it is applied.
    pub fn requested_marks(&self) -> MarkCounts {
        let mut counts = MarkCounts::default();
        MarkCounts::add(
            &self.reviewed,
            &mut counts.reviewed_lines,
            &mut counts.reviewed_files,
        );
        MarkCounts::add(
            &self.not_relevant,
            &mut counts.not_relevant_lines,
            &mut counts.not_relevant_files,
        );
        MarkCounts::add(
            &self.reopened,
            &mut counts.reopened_lines,
            &mut counts.reopened_files,
        );
        counts
    }
}

impl MarkCounts {
    fn add<'a>(
        locations: impl IntoIterator<Item = &'a crate::CodeLocation>,
        lines: &mut u32,
        files: &mut u32,
    ) {
        for location in locations {
            match &location.lines {
                Some(range) => *lines += range.count(),
                None => *files += 1,
            }
        }
    }
}

impl TurnMarks {
    pub fn counts(&self) -> MarkCounts {
        let mut counts = MarkCounts::default();
        MarkCounts::add(
            &self.reviewed,
            &mut counts.reviewed_lines,
            &mut counts.reviewed_files,
        );
        MarkCounts::add(
            &self.not_relevant,
            &mut counts.not_relevant_lines,
            &mut counts.not_relevant_files,
        );
        MarkCounts::add(
            self.reopened.iter().map(|reopened| &reopened.location),
            &mut counts.reopened_lines,
            &mut counts.reopened_files,
        );
        counts
    }
}

impl ExploreRound {
    pub fn new(exploration: Exploration) -> Self {
        Self {
            revision: 0,
            exploration,
            last_agent_session: None,
            turns: BTreeMap::new(),
            implementations: BTreeMap::new(),
            completion: None,
            marks: BTreeMap::new(),
        }
    }

    /// Validate on a candidate so rejection retains the pending request and
    /// answer; whether the update was new.
    pub fn submit(&mut self, update: &InterviewUpdate) -> eyre::Result<bool> {
        let mut candidate = self.exploration.clone();
        let applied = candidate.submit(update.clone())?;
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
        }
        Ok(applied)
    }

    /// Accept an already attributed UI contribution against the latest stored state.
    pub fn post(&mut self, request: &TurnRequest) -> eyre::Result<bool> {
        eyre::ensure!(
            request.instance == self.exploration.instance
                && request.checkpoint == self.exploration.comparison.checkpoint,
            "This answer belongs to another Explore round"
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
            in_reply_to: Some(answer.in_reply_to.clone()),
        });
        let validated = candidate.request(
            input,
            request
                .answer
                .as_ref()
                .and_then(|answer| answer.question.as_ref()),
        )?;
        eyre::ensure!(
            validated.cancelled == request.cancelled,
            "This turn does not name the answers cancelled since the last one"
        );
        self.exploration.cancelled.clear();
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
            self.completion.is_some(),
            "Explore has not accepted a conclusion"
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
        self.implementations.insert(
            request.delivery.clone(),
            ImplementationDelivery {
                authorized_at: self.revision + 1,
                attempt: uuid::Uuid::new_v4().to_string(),
                request: request.clone(),
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
        self.last_agent_session = ConversationBinding::from_agent(agent);
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
