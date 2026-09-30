//! One Explore session: the pass, its MCP access, the pinned agent and delivery attempts.
//!
//! The owner forwards every [`Input`] and reports review checkpoint changes; the
//! session publishes its results as application events and prompts agents through
//! the agent port.

mod agent;
mod dispatch;
mod implementation;
mod restore;
mod significance;
mod submission;
mod turn;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use component_core::ApplicationEventSender;
use herdr_client::protocol::{AgentPort, AgentTarget};
use review_explore::{Command, Comparison, ExclusionPolicy, ExplorePass, ViewSave};
use review_repository::repository::Repository;
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

    fn deliver(&self, input: Input) {
        (self.0)(input);
    }
}

/// What a session needs from the process that owns it.
pub struct Collaborators {
    pub repository: Repository,
    pub store: ReviewStore,
    pub agents: Arc<dyn AgentPort>,
    pub target: AgentTarget,
    pub prompts: PromptSender,
    pub exclusion: ExclusionPolicy,
    pub events: ApplicationEventSender,
    pub inbox: Inbox,
}

/// The Explore session of one reviewer process.
pub struct ExploreSession {
    repository: Repository,
    store: ReviewStore,
    agents: Arc<dyn AgentPort>,
    target: AgentTarget,
    prompts: PromptSender,
    exclusion: ExclusionPolicy,
    events: ApplicationEventSender,
    inbox: Inbox,
    state: State,
}

#[derive(Debug, Default)]
struct State {
    comparison: Option<Arc<Comparison>>,
    pass: Option<ExplorePass>,
    loaded_unit: Option<ReviewUnit>,
    historical: bool,
    storage_error: Option<String>,
    access: String,
    last_view: Option<ViewSave>,
    pending: Option<(String, String)>,
    agent: Option<PinnedAgent>,
    prompt: Option<PromptCancellation>,
    implementation: Option<PromptCancellation>,
    classification: Option<(String, Arc<AtomicBool>)>,
}

impl State {
    /// Revoke the MCP access of every earlier prompt.
    fn renew_access(&mut self) {
        self.access = uuid::Uuid::new_v4().to_string();
    }
}

/// Publish a durable pass change nobody waits to acknowledge; false once the
/// application stopped receiving events.
fn publish_committed(events: &ApplicationEventSender, pass: ExplorePass) -> bool {
    let (response, _) = std::sync::mpsc::channel();
    events
        .send(ui_events::ExploreCommitted {
            pass: Arc::new(pass),
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
            .field("exclusion", &self.exclusion)
            .finish_non_exhaustive()
    }
}

impl ExploreSession {
    pub fn new(collaborators: Collaborators) -> Self {
        let Collaborators {
            repository,
            store,
            agents,
            target,
            prompts,
            exclusion,
            events,
            inbox,
        } = collaborators;
        Self {
            repository,
            store,
            agents,
            target,
            prompts,
            exclusion,
            events,
            inbox,
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
    }

    /// The reviewer now shows `unit`; restore its latest pass when the unit changed.
    pub fn checkpoint_changed(&mut self, unit: &ReviewUnit) {
        if self.state.loaded_unit.as_ref() == Some(unit) {
            return;
        }
        self.state = State {
            loaded_unit: Some(unit.clone()),
            ..State::default()
        };
        self.open();
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Start => self.start(),
            Command::SaveView(view) => self.save_view(*view),
            Command::Turn(request) => self.deliver_turn(*request, None),
            Command::Retry(request) => self.retry(*request),
            Command::Implement(request) => self.implement(request),
            Command::RequireReview(units) => self.require_review(*units),
            Command::CancelImplementation => self.state.implementation = None,
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
            self.state.pass = None;
            self.state.historical = false;
            self.state.renew_access();
        }
        let _ = self.events.send(ui_events::ExploreCaptured {
            result: result.map_err(|error| error.to_string()),
        });
    }

    fn capture(&self) -> eyre::Result<Arc<Comparison>> {
        let review_repository::repository::PollResult::Complete(snapshot) =
            self.repository.poll()?
        else {
            eyre::bail!("Repository comparison is not ready; retry Start");
        };
        Ok(Arc::new(Comparison::prepare(&self.repository, &snapshot)?))
    }

    fn require_review(&mut self, units: Vec<review_explore::CoverageUnit>) {
        let Some(pass) = &self.state.pass else {
            return;
        };
        if self.state.historical || pass.completion.is_some() {
            return;
        }
        let unit = pass.exploration.comparison.checkpoint.review_unit.clone();
        let instance = pass.exploration.instance.clone();
        let result = self.store.update_explore(&unit, &instance, |pass| {
            if pass.completion.is_some() {
                return Err("Explore is already finalizing".into());
            }
            let excluded = pass.coverage.unexplored_exclusions();
            if !units.iter().all(|unit| excluded.contains(unit)) {
                return Err("Exclusion is no longer current".into());
            }
            pass.coverage.require_review(units);
            Ok(())
        });
        match result {
            Ok(((), pass)) => {
                self.state.pass = Some(pass.clone());
                publish_committed(&self.events, pass);
            }
            Err(error) => {
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
            }
        }
    }

    fn cancel_record(&mut self) -> bool {
        let Some(pass) = &self.state.pass else {
            return true;
        };
        if self.state.historical {
            return true;
        }
        if let Err(error) = self.store.update_explore(
            &pass.exploration.comparison.checkpoint.review_unit,
            &pass.exploration.instance,
            |pass| {
                pass.exploration.cancel();
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
