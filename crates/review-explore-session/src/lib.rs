//! One Explore session: the round, its MCP access, the pinned agent and delivery attempts.
//!
//! The owner forwards every [`Input`] and reports review checkpoint changes; the
//! session publishes its results as application events and prompts agents through
//! the agent port.

mod agent;
mod cancel;
mod dispatch;
mod implementation;
mod marks;
mod page;
mod records;
mod restore;
mod submission;
mod turn;
mod turn_log;
mod unreviewed;
mod unreviewed_diffs;

pub use turn_log::TurnLog;

use std::sync::Arc;

use component_core::ApplicationEventSender;
use herdr_client::protocol::{AgentPort, AgentTarget};
use review_explore::{Command, Comparison, ExploreRound, ViewSave};
use review_explore_page::RoundPublisher;
use review_repository::repository::Repository;
use review_state::ReviewTracker;
use review_store::ReviewStore;
use review_thread_service::{PinnedAgent, PromptCancellation, PromptSender};
use review_types::ReviewUnit;

/// Work the session's owner forwards, in the order it arrived.
#[derive(Debug)]
pub enum Input {
    /// An explicit reviewer command.
    Command(Command),
    /// An agent's MCP call to an Explore tool.
    Submission(Box<review_mcp::Request>),
    /// A prompt this session sent could not be delivered.
    PromptFinished {
        event: Box<ui_events::ExploreFinished>,
        attempt: String,
    },
    /// Saved Explore state changed on disk, possibly by another reviewer.
    StorageChanged,
}

/// Returns inputs the session produces later, such as prompt outcomes, to its owner's
/// serial input order.
#[derive(Clone)]
pub struct Inbox(Arc<dyn Fn(Input) + Send + Sync>);

impl Inbox {
    pub fn new(deliver: impl Fn(Input) + Send + Sync + 'static) -> Self {
        Self(Arc::new(deliver))
    }

    /// Hand `input` to the owner, behind the inputs it already holds.
    pub fn deliver(&self, input: Input) {
        (self.0)(input);
    }
}

/// What a session needs from the process that owns it.
pub struct Collaborators {
    pub repository: Repository,
    pub store: ReviewStore,
    /// The review marks every reviewer component shares.
    pub tracker: Arc<ReviewTracker>,
    pub agents: Arc<dyn AgentPort>,
    pub target: AgentTarget,
    pub prompts: PromptSender,
    pub events: ApplicationEventSender,
    pub inbox: Inbox,
    /// Where a `make vision` session reads the prompts this session sent;
    /// `None` in every other reviewer.
    pub turns: Option<TurnLog>,
    /// Where the session publishes the stage of its round for the Explore page.
    pub page: RoundPublisher,
}

/// The Explore session of one reviewer process.
pub struct ExploreSession {
    repository: Repository,
    tracker: Arc<ReviewTracker>,
    rounds: records::SavedRounds,
    agents: Arc<dyn AgentPort>,
    target: AgentTarget,
    prompts: PromptSender,
    events: ApplicationEventSender,
    inbox: Inbox,
    turns: Option<TurnLog>,
    page: RoundPublisher,
    /// The citations of the question the page shows.
    citations: page::PageCitations,
    /// The unreviewed lines of the latest prompt, as files.
    diffs: Option<unreviewed_diffs::UnreviewedDiffs>,
    state: State,
}

#[derive(Debug, Default)]
struct State {
    comparison: Option<Arc<Comparison>>,
    round: Option<ExploreRound>,
    loaded_unit: Option<ReviewUnit>,
    historical: bool,
    storage_error: Option<String>,
    access: String,
    last_view: Option<ViewSave>,
    pending: Option<(String, String)>,
    /// The latest round when the reviewer reset another one; it stays out of view.
    dismissed: Option<String>,
    agent: Option<PinnedAgent>,
    prompt: Option<PromptCancellation>,
    implementation: Option<PromptCancellation>,
}

impl State {
    /// Revoke the MCP access of every earlier prompt.
    fn renew_access(&mut self) {
        self.access = uuid::Uuid::new_v4().to_string();
    }
}

/// Publish a durable round change nobody waits to acknowledge; false once the
/// application stopped receiving events.
fn publish_committed(events: &ApplicationEventSender, round: ExploreRound) -> bool {
    let (response, _) = std::sync::mpsc::channel();
    events
        .send(ui_events::ExploreCommitted {
            round: Arc::new(round),
            applied: true,
            response,
        })
        .is_ok()
}

impl std::fmt::Debug for ExploreSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExploreSession")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl ExploreSession {
    pub fn new(collaborators: Collaborators) -> Self {
        let Collaborators {
            repository,
            store,
            tracker,
            agents,
            target,
            prompts,
            events,
            inbox,
            turns,
            page,
        } = collaborators;
        Self {
            repository,
            tracker,
            rounds: records::SavedRounds::new(store),
            agents,
            target,
            prompts,
            events,
            inbox,
            turns,
            page,
            citations: page::PageCitations::default(),
            diffs: None,
            state: State::default(),
        }
    }

    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Command(command) => self.command(command),
            Input::Submission(request) => self.submission(*request),
            Input::PromptFinished { event, attempt } => self.prompt_finished(*event, &attempt),
            Input::StorageChanged => self.storage_changed(),
        }
        self.publish_page();
    }

    /// The reviewer now shows `unit`; restore its latest round when the unit changed.
    pub fn checkpoint_changed(&mut self, unit: &ReviewUnit) {
        if self.state.loaded_unit.as_ref() == Some(unit) {
            return;
        }
        self.state = State {
            loaded_unit: Some(unit.clone()),
            ..State::default()
        };
        self.open();
        self.publish_page();
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Start => self.start(),
            Command::Reset => self.reset(),
            Command::SaveView(view) => self.save_view(*view),
            Command::Turn(request) => self.deliver_turn(*request, None),
            Command::Retry(request) => self.retry(*request),
            Command::Implement(request) => self.implement(request),
            Command::CancelImplementation => self.state.implementation = None,
            Command::CancelAnswer(answer) => self.cancel_answer(answer),
            Command::Cancel => {
                self.cancel_record();
                self.state.renew_access();
                self.state.prompt = None;
                self.state.pending = None;
                self.state.implementation = None;
            }
        }
    }

    fn start(&mut self) {
        if self.state.storage_error.is_some() || !self.cancel_record() {
            let _ = self.events.send(ui_events::ExploreCaptured {
                result: Err(self
                    .state
                    .storage_error
                    .clone()
                    .unwrap_or_else(|| "Explore could not save its state".into())),
            });
            return;
        }
        self.state.prompt = None;
        self.state.implementation = None;
        self.state.pending = None;
        let result = self.capture();
        if let Ok(comparison) = &result {
            self.state.comparison = Some(comparison.clone());
            self.state.agent = None;
            self.state.round = None;
            self.state.historical = false;
            self.state.renew_access();
        }
        let _ = self.events.send(ui_events::ExploreCaptured {
            result: result.map_err(|error| error.to_string()),
        });
    }

    /// Close the round and forget it: reopening shows the start screen. A later
    /// round another reviewer started stays theirs, and out of view until reopening.
    fn reset(&mut self) {
        self.cancel_record();
        let closed = self.state.round.as_ref().map_or(Ok(()), |round| {
            self.rounds.close(
                &round.exploration.comparison.checkpoint.review_unit,
                &round.exploration.instance,
            )
        });
        let dismissed = self.state.loaded_unit.as_ref().and_then(|unit| {
            let history = self.rounds.history(unit).ok()?;
            history.restorable().map(str::to_owned)
        });
        self.state = State {
            loaded_unit: self.state.loaded_unit.take(),
            storage_error: self.state.storage_error.take(),
            dismissed,
            ..State::default()
        };
        self.state.renew_access();
        if let Err(error) = closed {
            self.state.storage_error = Some(error.to_string());
            let _ = self
                .events
                .send(ui_events::ExploreStorageFailed(error.to_string()));
        }
    }

    fn capture(&self) -> eyre::Result<Arc<Comparison>> {
        let review_repository::repository::PollResult::Complete(snapshot) =
            self.repository.poll()?
        else {
            eyre::bail!("Repository comparison is not ready; retry Start");
        };
        Ok(Arc::new(Comparison::prepare(&self.repository, &snapshot)?))
    }

    fn cancel_record(&mut self) -> bool {
        let Some(round) = &self.state.round else {
            return true;
        };
        if self.state.historical {
            return true;
        }
        if let Err(error) = self.rounds.update(
            &round.exploration.comparison.checkpoint.review_unit,
            &round.exploration.instance,
            |round| {
                round.exploration.cancel();
                Ok(())
            },
        ) {
            self.state.storage_error = Some(error.to_string());
            let _ = self
                .events
                .send(ui_events::ExploreStorageFailed(error.to_string()));
            return false;
        }
        true
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
