//! One Explore session: the round, its MCP access, the pinned agent and delivery attempts.
//!
//! The owner forwards every [`Input`] and reports review checkpoint changes; the
//! session publishes its results as application events and prompts agents through
//! the agent port.

mod agent;
mod cancel;
mod diagram;
mod dispatch;
mod implementation;
mod mark_tally;
mod marks;
mod page;
mod page_actions;
mod page_save;
mod quiz;
mod records;
mod restore;
mod run_ahead;
mod start_block;
mod submission;
mod turn;
mod turn_log;
mod unreviewed;
mod unreviewed_diffs;

pub use run_ahead::RunAheadInput;
pub use turn_log::TurnLog;

use std::sync::Arc;

use component_core::ApplicationEventSender;
use herdr_client::protocol::{AgentPort, AgentTarget};
use review_explore::{Command, Comparison, Exploration, ExploreRound, TurnRequest, ViewSave};
use review_explore_page::{CommandRefusal, CommandReply, RoundPublisher};
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
    /// A command the reviewer sent from the Explore page, and where to reply once it is
    /// carried out or refused.
    Page {
        command: review_explore_page::PageCommand,
        reply: review_explore_page::CommandReply,
    },
    /// An event of run-ahead's forks, or of the agent they are forked from.
    RunAhead(RunAheadInput),
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
    /// The agent whose session run-ahead forks.
    pub forks: Arc<dyn review_run_ahead::ForkHost>,
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
    /// The citations of the earlier questions the page shows from the round rail.
    earlier_citations: page::PageCitations,
    /// The review marks of the reviewer's snapshot, as the session read them last.
    marks: review_explore_tally::ChangeMarks,
    /// The unreviewed lines of the latest prompt, as files.
    diffs: Option<unreviewed_diffs::UnreviewedDiffs>,
    /// Why no round can start, as the latest review marks the session read say.
    start_block: Option<review_explore::StartBlock>,
    /// The forks of the question that waits.
    run_ahead: run_ahead::RunAheadState,
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
    /// Where the reviewer's latest start of a round stands, while no round runs.
    start: Start,
    agent: Option<PinnedAgent>,
    prompt: Option<PromptCancellation>,
    implementation: Option<PromptCancellation>,
}

/// Where the reviewer's latest start of a round stands, while the session has no round. Each
/// start has an identity, made before the reviewer starts it: the page shows it with the start
/// screen, and a Start from the page carries it back, so that a late repeat of a start that
/// went through, or was stopped since, starts nothing.
#[derive(Debug)]
enum Start {
    /// No start is under way, and the latest one, if any, succeeded; `offer` is the next one.
    Idle { offer: String },
    /// The start `start`, which the reviewer started at `started_at_ms`, in milliseconds since
    /// the epoch: the change is captured, or being captured, and the new round's kickoff is not
    /// saved yet.
    Starting { start: String, started_at_ms: u64 },
    /// The latest start failed, for `failure`; `offer` is the next one.
    Failed { failure: String, offer: String },
}

impl Default for Start {
    fn default() -> Self {
        Self::Idle {
            offer: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl Start {
    /// The start the session offers, while none is under way.
    fn offered(&self) -> Option<&str> {
        match self {
            Self::Idle { offer } | Self::Failed { offer, .. } => Some(offer),
            Self::Starting { .. } => None,
        }
    }

    /// The start under way, if any.
    fn starting(&self) -> Option<&str> {
        match self {
            Self::Starting { start, .. } => Some(start),
            _ => None,
        }
    }

    /// The offered start begins; a start under way goes on.
    fn begin(&mut self) {
        if let Some(offer) = self.offered() {
            *self = Self::Starting {
                start: offer.to_owned(),
                started_at_ms: review_explore::now_ms(),
            };
        }
    }

    /// The start under way ended with its round, or was stopped: the next one is offered. A
    /// failed start stays failed.
    fn settle(&mut self) {
        if self.starting().is_some() {
            *self = Self::default();
        }
    }
}

impl State {
    /// The start under way failed, for `error`.
    fn start_failed(&mut self, error: &dyn std::fmt::Display) {
        self.start = Start::Failed {
            failure: error.to_string(),
            offer: uuid::Uuid::new_v4().to_string(),
        };
    }

    /// This process delivers the turn `request` to the agent from now on.
    fn pend(&mut self, request: &TurnRequest) {
        self.pending = Some((request.instance.clone(), request.request.clone()));
    }

    /// Whether this process delivers the turn `request` of the round `instance` to the agent.
    fn is_pending(&self, instance: &str, request: &str) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(pending_instance, pending)| {
                pending_instance == instance && pending == request
            })
    }

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

impl Drop for ExploreSession {
    fn drop(&mut self) {
        self.run_ahead_close();
    }
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
            forks,
        } = collaborators;
        Self {
            repository,
            tracker,
            marks: review_explore_tally::ChangeMarks::new(store.clone()),
            rounds: records::SavedRounds::new(store),
            agents,
            target,
            prompts,
            events,
            inbox,
            turns,
            page,
            citations: page::PageCitations::default(),
            earlier_citations: page::PageCitations::default(),
            diffs: None,
            start_block: None,
            run_ahead: run_ahead::RunAheadState::new(forks),
            state: State::default(),
        }
    }

    pub fn handle(&mut self, input: Input) {
        match input {
            Input::Command(command) => self.command(command),
            Input::Submission(request) => self.submission(*request),
            Input::PromptFinished { event, attempt } => self.prompt_finished(*event, &attempt),
            Input::StorageChanged => self.storage_changed(),
            Input::Page { command, reply } => self.page_command(command, reply),
            Input::RunAhead(input) => self.run_ahead_input(input),
        }
        self.run_ahead_reconcile();
        self.publish_page();
    }

    /// Whether a round runs: the start screen, and whether it can start a round, do not show.
    pub fn runs_round(&self) -> bool {
        self.state.round.is_some()
    }

    /// The reviewer now shows `unit`; restore its latest round when the unit changed.
    pub fn checkpoint_changed(&mut self, unit: &ReviewUnit) {
        if self.state.loaded_unit.as_ref() == Some(unit) {
            return;
        }
        self.run_ahead_discard(review_run_ahead::DiscardReason::ReviewChanged);
        self.state = State {
            loaded_unit: Some(unit.clone()),
            ..State::default()
        };
        self.open();
        self.run_ahead_reconcile();
        self.publish_page();
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Start => self.start(),
            Command::Reset => self.reset(),
            Command::SaveView(view) => self.save_view(*view),
            // The pane learns the outcome from the event the session sends.
            Command::Turn(request) => {
                let _ = self.deliver_turn(*request, None);
            }
            // The pane learns the outcome from the events the session sends.
            Command::Retry(request) => {
                let _ = self.retry(*request);
            }
            // The pane learns the outcome from the events the session sends.
            Command::Implement(request) => {
                let _ = self.implement(request);
            }
            Command::CancelImplementation => self.state.implementation = None,
            // The pane learns the outcome from the event the session sends.
            Command::CancelAnswer(answer) => {
                let _ = self.cancel_answer(answer);
            }
            Command::Cancel => {
                self.cancel_record();
                self.state.start.settle();
                self.state.renew_access();
                self.state.prompt = None;
                self.state.pending = None;
                self.state.implementation = None;
            }
        }
    }

    fn start(&mut self) {
        if self.state.storage_error.is_some() || !self.cancel_record() {
            let error = self
                .state
                .storage_error
                .clone()
                .unwrap_or_else(|| "Explore could not save its state".into());
            self.state.start_failed(&error);
            let _ = self
                .events
                .send(ui_events::ExploreCaptured { result: Err(error) });
            return;
        }
        self.state.prompt = None;
        self.state.implementation = None;
        self.state.pending = None;
        let result = self.capture();
        match &result {
            Ok(comparison) => self.begin_start(comparison.clone()),
            Err(error) => self.state.start_failed(error),
        }
        let _ = self.events.send(ui_events::ExploreCaptured {
            result: result.map_err(|error| error.to_string()),
        });
    }

    /// Starts a round from the Explore page, as Start or Start with Challenger in the pane
    /// would, and returns its kickoff for the owner to send, after Jev's marks when Jev is
    /// enabled. Refuses while a round runs or starts, and when nothing is left to review. It
    /// replies to the page before it captures the change, so that the page shows the round
    /// starting; a capture that fails shows on the page as a failed start. The pane hears of the
    /// start, and of its failure.
    pub fn start_from_page(
        &mut self,
        challenger: bool,
        start: &str,
        reply: CommandReply,
    ) -> Option<TurnRequest> {
        let refusal = if self.state.round.is_none() && self.state.start.starting() == Some(start) {
            // A repeat of the start under way.
            Some(CommandRefusal::AlreadyApplied)
        } else if self.state.round.is_some() || self.state.start.offered() != Some(start) {
            Some(CommandRefusal::Stale)
        } else if let Some(block) = self.start_block {
            Some(CommandRefusal::Failed(block.reason().into()))
        } else {
            self.state.storage_error.clone().map(CommandRefusal::Failed)
        };
        if let Some(refusal) = refusal {
            reply.send(Err(refusal));
            return None;
        }
        self.state.start.begin();
        self.publish_page();
        reply.send(Ok(()));
        let _ = self.events.send(ui_events::ExplorePageStart(Ok(())));
        let kickoff = self.capture().and_then(|comparison| {
            self.begin_start(comparison.clone());
            let mut exploration = Exploration::new(comparison);
            exploration.challenger = challenger;
            exploration.writing = self.rounds.next_writing()?;
            exploration.request(None, None)
        });
        match kickoff {
            Ok(kickoff) => Some(kickoff),
            Err(error) => {
                let _ = self
                    .events
                    .send(ui_events::ExplorePageStart(Err(error.to_string())));
                self.state.start_failed(&error);
                self.publish_page();
                None
            }
        }
    }

    /// Takes `comparison`, the change a start captured, for the kickoff to come: the earlier
    /// round's agent and access no longer apply.
    fn begin_start(&mut self, comparison: Arc<Comparison>) {
        self.run_ahead_discard(review_run_ahead::DiscardReason::NewRound);
        self.state.comparison = Some(comparison);
        self.state.agent = None;
        self.state.round = None;
        self.state.historical = false;
        self.state.start.begin();
        self.state.renew_access();
    }

    /// Close the round and forget it: reopening shows the start screen. A later
    /// round another reviewer started stays theirs, and out of view until reopening.
    fn reset(&mut self) {
        self.run_ahead_discard(review_run_ahead::DiscardReason::Reset);
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

    /// Captures the change for a new round, unless nothing is left to review in it.
    fn capture(&mut self) -> eyre::Result<Arc<Comparison>> {
        let review_repository::repository::PollResult::Complete(snapshot) =
            self.repository.poll()?
        else {
            eyre::bail!("Repository comparison is not ready; retry Start");
        };
        if let Some(block) = self.refresh_start_block(&snapshot) {
            eyre::bail!("{block}");
        }
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
