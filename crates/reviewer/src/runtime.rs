//! Terminal and worker integration for the review pane.

use review_thread_service as comments;
mod actions;
mod auto_review;
#[path = "runtime/comments.rs"]
mod comment_service;
mod document;
mod editor;
mod events;
mod highlighting;
mod jev;
mod review_marks;
mod terminal;
mod timing;

#[cfg(all(test, unix))]
#[path = "runtime/responsiveness.tests.rs"]
mod responsiveness;

use std::env;
use std::io::{self, stdout};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use component_core::{ApplicationEventSender, EventEnvelope};
#[cfg(test)]
use crossbeam_channel::Receiver as EventReceiver;
use crossbeam_channel::{Sender as EventSender, unbounded};
use crossterm::event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event,
    KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{AgentTarget, HerdrEvent, PaneId, PluginContext, WorkspaceId};
use ratatui::Terminal;
use ratatui::backend::Backend;
use review_explore::ExclusionPolicy;
use review_explore_session::{self as explore_session, ExploreSession};
use review_lsp::SourceLocation;
use review_repository::diff::parse_file_diff;
use review_repository::repository::{ChangeId, ChangedFile, PollResult, Repository, Snapshot};
use review_source::ReviewCheckpoint;
use review_state::{MarkResult, ReviewTracker};
use review_store::ReviewStore;
use review_ui::{
    Action, DocumentAction, DocumentLoad, Key, LspAction, RepositoryAction, ReviewApplication,
    SettingsAction, SourceLoadMode, TerminalAction, Theme, UserInput,
};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::flag;
use terminal::{CursorBackend, TerminalBackend};
use ui_events::{
    AnimationTick, DiffContentLoadFailed, DiffContentLoaded, FileSummary, RepositoryFilesChanged,
    RepositoryMetadataChanged, RepositoryRefreshFinished, RepositoryRefreshStarted,
    ReviewStateSaved, RevisionCandidatesLoaded, RevisionEditFailed, RevisionHistoryLoaded,
    SourceContentLoadFailed, SourceContentLoaded, ToastExpirationTick,
};

use crate::watcher::RepositoryWatcher;
use actions::ActionExecutors;

const TIMER_INTERVAL: Duration = Duration::from_millis(50);
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);
const HERDR_EVENT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const EVENT_BATCH_LIMIT: usize = 64;
const EVENT_BATCH_BUDGET: Duration = Duration::from_millis(8);

/// Route inputs the Explore session produces later back to the worker's serial order.
fn explore_inbox(commands: Sender<WorkerCommand>) -> explore_session::Inbox {
    explore_session::Inbox::new(move |input| {
        let _ = commands.send(WorkerCommand::Explore(input));
    })
}

/// The running review pane.
#[derive(Debug)]
pub struct Runtime {
    repository: Repository,
    state_dir: PathBuf,
    workspace_id: WorkspaceId,
    initial_agent: Option<PaneId>,
    client: HerdrClient,
    theme: Theme,
}

#[derive(Debug)]
struct Worker {
    repository: Repository,
    tracker: Arc<ReviewTracker>,
    store: ReviewStore,
    snapshot: Option<Snapshot>,
    commands: Sender<WorkerCommand>,
    explore: ExploreSession,
    exclusion: ExclusionPolicy,
    auto_review: Option<Arc<AtomicBool>>,
    documents: Sender<document::Command>,
}

#[derive(Debug)]
enum WorkerCommand {
    Explore(explore_session::Input),
    Repository(RepositoryAction),
    Poll,
    AutoReviewFinished(Box<auto_review::AutoReview>),
    Quit,
}

struct TerminalGuard {
    terminal: Terminal<TerminalBackend<io::Stdout>>,
}

struct TerminalEventProducer {
    events: EventSender<EventEnvelope>,
    stop_requested: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct RuntimeEventProducers {
    stop_requested: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

struct RuntimeEventLoop<'a, B: Backend> {
    target: AgentTarget,
    source_watches: Option<&'a crate::watcher::SourceWatchRequests>,
    terminal_events: Option<&'a mut TerminalEventProducer>,
    last_frame: Instant,
    comments: &'a comments::Worker,
    terminal: &'a mut Terminal<B>,
    app: &'a mut ReviewApplication,
    commands: &'a Sender<WorkerCommand>,
    documents: &'a Sender<document::Command>,
    search: &'a text_search::Worker,
    highlighting: &'a highlighting::Worker,
    events: &'a mut events::Inbox,
    timings: &'a timing::Recorder,
    lsp: &'a review_lsp::Worker,
    repository_root: &'a Path,
    settings: &'a ReviewStore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlEventOutcome {
    NotHandled,
    Continue,
    Stop,
}

struct RuntimeActionDispatcher<'a> {
    source_watches: Option<&'a crate::watcher::SourceWatchRequests>,
    comments: &'a comments::Worker,
    commands: &'a Sender<WorkerCommand>,
    documents: &'a Sender<document::Command>,
    search: &'a text_search::Worker,
    highlighting: &'a highlighting::Worker,
    settings: &'a ReviewStore,
    repository_root: &'a Path,
    lsp: &'a review_lsp::Worker,
    open_in_editor: &'a mut dyn FnMut(&Path, Option<u32>) -> eyre::Result<()>,
}

struct BackgroundWorkers {
    target: AgentTarget,
    comments: comments::Worker,
    search: text_search::Worker,
    highlighting: highlighting::Worker,
    commands: Sender<WorkerCommand>,
    documents: Sender<document::Command>,
    threads: [JoinHandle<()>; 2],
}

impl BackgroundWorkers {
    fn stop(self) {
        drop(self.comments);
        drop(self.search);
        drop(self.highlighting);
        let _ = self.commands.send(WorkerCommand::Quit);
        let _ = self.documents.send(document::Command::Quit);
        for worker in self.threads {
            let _ = worker.join();
        }
    }
}

struct WorkerStopped;
struct StopRequested;
struct RepositoryRefreshDue;
struct ApplicationTick(Instant);
struct TerminalFailed(String);
struct TerminalFocused;

#[cfg(test)]
struct ApplicationMessageReceiver(EventReceiver<EventEnvelope>);

#[cfg(test)]
impl ApplicationMessageReceiver {
    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<EventEnvelope, crossbeam_channel::RecvTimeoutError> {
        self.0.recv_timeout(timeout)
    }

    fn try_iter(&self) -> impl Iterator<Item = EventEnvelope> + '_ {
        self.0.try_iter()
    }
}

#[cfg(test)]
fn application_message_channel() -> (ApplicationEventSender, ApplicationMessageReceiver) {
    let (sender, receiver) = unbounded();
    (
        ApplicationEventSender::new(sender),
        ApplicationMessageReceiver(receiver),
    )
}

#[derive(Default)]
struct MouseClicks {
    previous: Option<(u16, u16, Instant)>,
}

impl Runtime {
    /// Read the pane context supplied by Herdr.
    pub fn from_env() -> eyre::Result<Self> {
        let state_dir = env::var_os("HERDR_PLUGIN_STATE_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| eyre::eyre!("HERDR_PLUGIN_STATE_DIR is not set"))?;
        let repository = Repository::discover(env::current_dir()?)?.with_state_root(&state_dir);
        let context: PluginContext = serde_json::from_str(
            &env::var("HERDR_PLUGIN_CONTEXT_JSON")
                .map_err(|_| eyre::eyre!("HERDR_PLUGIN_CONTEXT_JSON is not set"))?,
        )?;
        Ok(Self {
            repository,
            state_dir,
            workspace_id: WorkspaceId(
                env::var("HERDR_WORKSPACE_ID")
                    .map_err(|_| eyre::eyre!("HERDR_WORKSPACE_ID is not set"))?,
            ),
            initial_agent: context.focused_pane_id,
            client: HerdrClient::from_env()?,
            theme: Theme::from_env()?,
        })
    }

    /// Run until the user quits or Herdr stops the pane.
    pub fn run(self) -> eyre::Result<()> {
        let timings = timing::Recorder::from_env()?;
        let stopped = Arc::new(AtomicBool::new(false));
        Self::register_stop_signals(&stopped)?;

        let settings = ReviewStore::open(&self.state_dir, self.repository.root())?;
        let file_pane_width = settings.file_pane_width()?;
        let root = self.repository.root().to_owned();
        let mut terminal = TerminalGuard::new()?;
        let mut app = ReviewApplication::new(self.theme, file_pane_width, root.clone());
        app.set_editor_keymap(settings.editor_keymap()?);
        let area = terminal.terminal.size()?;
        let _ = app.update(UserInput::Resize {
            width: area.width,
            height: area.height,
        });
        terminal
            .terminal
            .draw(|frame| frame.render_widget(app.frame(), frame.area()))?;

        let (event_sender, events) = unbounded();
        let (input_sender, inputs) = unbounded();
        let workers = self.start_workers(event_sender.clone(), input_sender.clone())?;
        let commands = &workers.commands;
        let producer_stop_requested = Arc::new(AtomicBool::new(false));
        let mut event_producers = RuntimeEventProducers::new(Arc::clone(&producer_stop_requested));
        event_producers.push(Self::start_herdr_events(
            self.client.clone(),
            event_sender.clone(),
            Arc::clone(&producer_stop_requested),
        ));
        let mut terminal_events = TerminalEventProducer::start(input_sender);
        let watcher = RepositoryWatcher::new(self.repository.root(), self.repository.repo_type());
        let changed = workers.commands.clone();
        let explore_events = event_sender.clone();
        if let Err(error) = settings.prepare_explore_storage() {
            let _ = explore_events.send(EventEnvelope::new(ui_events::ExploreStorageFailed(
                error.to_string(),
            )));
        } else {
            watcher.watch_explore(settings.explore_directory(), move |event| match event {
                Ok(()) => {
                    let _ = changed.send(WorkerCommand::Explore(
                        explore_session::Input::StorageChanged,
                    ));
                }
                Err(error) => {
                    let _ = explore_events.send(EventEnvelope::new(
                        ui_events::ExploreStorageFailed(format!("Watch Explore state: {error}")),
                    ));
                }
            });
        }
        let source_watches = watcher.source_requests();
        event_producers.push(Self::start_periodic_events(
            event_sender.clone(),
            Arc::clone(&stopped),
            Arc::clone(&producer_stop_requested),
            watcher,
        ));
        let lsp = review_lsp::Worker::start(root.clone());
        event_producers.push(Self::start_lsp_events(
            &lsp,
            event_sender,
            producer_stop_requested,
        ));
        let _ = app.publish(RepositoryRefreshStarted);
        commands.send(WorkerCommand::Poll)?;
        let result = RuntimeEventLoop {
            target: workers.target.clone(),
            source_watches: Some(&source_watches),
            terminal_events: Some(&mut terminal_events),
            last_frame: Instant::now(),
            comments: &workers.comments,
            terminal: &mut terminal.terminal,
            app: &mut app,
            commands,
            documents: &workers.documents,
            search: &workers.search,
            highlighting: &workers.highlighting,
            events: &mut events::Inbox::new(events, inputs),
            timings: &timings,
            lsp: &lsp,
            repository_root: &root,
            settings: &settings,
        }
        .run();
        terminal_events.stop();
        event_producers.stop();
        self.repository.cancel();
        drop(terminal);
        workers.stop();
        result
    }

    fn register_stop_signals(stopped: &Arc<AtomicBool>) -> eyre::Result<()> {
        for signal in [SIGINT, SIGTERM, SIGHUP] {
            flag::register(signal, Arc::clone(stopped))?;
        }
        Ok(())
    }

    fn start_herdr_events(
        event_client: HerdrClient,
        events: EventSender<EventEnvelope>,
        stop_requested: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            while !stop_requested.load(Ordering::Relaxed) {
                match event_client.forward_events_while(
                    || !stop_requested.load(Ordering::Relaxed),
                    |event| events.send(EventEnvelope::new(event)).is_ok(),
                ) {
                    Ok(()) => return,
                    Err(_) => thread::sleep(HERDR_EVENT_RECONNECT_DELAY),
                }
            }
        })
    }

    fn start_lsp_events(
        lsp: &review_lsp::Worker,
        events: EventSender<EventEnvelope>,
        stop_requested: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let lsp_events = lsp.event_receiver();
        thread::spawn(move || {
            while !stop_requested.load(Ordering::Relaxed) {
                match lsp_events.recv_timeout(TIMER_INTERVAL) {
                    Ok(event) => {
                        if events.send(EventEnvelope::new(event)).is_err() {
                            return;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                }
            }
        })
    }

    fn start_periodic_events(
        events: EventSender<EventEnvelope>,
        stopped: Arc<AtomicBool>,
        producer_stop_requested: Arc<AtomicBool>,
        mut watcher: RepositoryWatcher,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            loop {
                if producer_stop_requested.load(Ordering::Relaxed) {
                    return;
                }
                let now = Instant::now();
                if stopped.load(Ordering::Relaxed) {
                    let _ = events.send(EventEnvelope::new(StopRequested));
                    return;
                }
                if watcher.take_failure() {
                    let _ = events.send(EventEnvelope::new(ui_events::ToastRequested {
                        text: "Filesystem watcher stopped; live updates are paused. Reopen the reviewer to retry.".into(),
                        kind: toasts::ToastKind::Error,
                    }));
                }
                if watcher.refresh_due(now)
                    && events
                        .send(EventEnvelope::new(RepositoryRefreshDue))
                        .is_err()
                {
                    return;
                }
                if events
                    .send(EventEnvelope::new(ApplicationTick(now)))
                    .is_err()
                {
                    return;
                }
                thread::sleep(TIMER_INTERVAL);
            }
        })
    }

    fn start_workers(
        &self,
        events: EventSender<EventEnvelope>,
        interactive: EventSender<EventEnvelope>,
    ) -> eyre::Result<BackgroundWorkers> {
        let target = AgentTarget::new(self.workspace_id.clone(), self.initial_agent.clone());
        let store = ReviewStore::open(&self.state_dir, self.repository.root())?;
        let tracker = Arc::new(ReviewTracker::new(self.repository.clone(), store.clone()));
        let (command_sender, command_receiver) = mpsc::channel();
        let (documents, document_receiver) = mpsc::channel();
        let mut document_worker = document::DocumentWorker {
            repository: self.repository.clone(),
            tracker: Arc::clone(&tracker),
            snapshot: None,
        };
        let document_messages = ApplicationEventSender::new(events.clone());
        let document_thread = thread::spawn(move || {
            document_worker.run(&document_receiver, &document_messages);
            let _ = document_messages.send(WorkerStopped);
        });
        let messages = ApplicationEventSender::new(events.clone());
        let comments = self.start_comments(
            store.clone(),
            messages.clone(),
            target.clone(),
            command_sender.clone(),
        );
        let exclusion = jev::exclusion_policy_from_env();
        let explore = ExploreSession::new(explore_session::Collaborators {
            repository: self.repository.clone(),
            store: store.clone(),
            agents: Arc::new(self.client.clone()),
            target: target.clone(),
            prompts: comments.prompt_sender(),
            exclusion: exclusion.clone(),
            events: messages.clone(),
            inbox: explore_inbox(command_sender.clone()),
        });
        let mut worker = Worker {
            repository: self.repository.clone(),
            tracker,
            store,
            snapshot: None,
            commands: command_sender.clone(),
            explore,
            exclusion,
            auto_review: None,
            documents: documents.clone(),
        };
        let handle = thread::spawn(move || {
            worker.run(&command_receiver, &messages);
            let _ = messages.send(WorkerStopped);
        });
        let highlight_events = events;
        let highlighting = highlighting::Worker::start(
            syntax_highlighting::SyntaxHighlighter::new(self.theme.syntax, self.theme.palette.text),
            move |result| {
                let _ = highlight_events.send(EventEnvelope::new(result));
            },
        );
        let search = text_search::Worker::start(move |results| {
            let _ = interactive.send(EventEnvelope::new(results));
        });
        Ok(BackgroundWorkers {
            target,
            comments,
            search,
            highlighting,
            commands: command_sender,
            documents,
            threads: [handle, document_thread],
        })
    }
}

impl ActionExecutors for RuntimeActionDispatcher<'_> {
    fn explore(&mut self, command: review_explore::Command) -> eyre::Result<()> {
        self.commands
            .send(WorkerCommand::Explore(explore_session::Input::Command(
                command,
            )))?;
        Ok(())
    }

    fn thread(&mut self, command: review_threads::ThreadCommand) -> eyre::Result<()> {
        self.comments.send(comments::Command::Thread(command));
        Ok(())
    }

    fn document(&mut self, action: DocumentAction) -> eyre::Result<()> {
        match action {
            // One command per path keeps each diff ordered against later snapshots.
            DocumentAction::Load(DocumentLoad::Diffs {
                review_checkpoint,
                paths,
            }) => {
                for path in paths {
                    self.documents
                        .send(document::Command::Load(DocumentLoad::Diff {
                            review_checkpoint: review_checkpoint.clone(),
                            path,
                        }))?;
                }
            }
            DocumentAction::Load(load) => self.documents.send(document::Command::Load(load))?,
            DocumentAction::Highlight(request) => self
                .highlighting
                .submit(request)
                .map_err(eyre::Report::msg)?,
            DocumentAction::Search(request) => self.search.submit(request),
            DocumentAction::WatchSource(path) => {
                if let Some(watches) = self.source_watches {
                    watches.watch(path.as_deref());
                }
            }
        }
        Ok(())
    }

    fn lsp(&mut self, action: LspAction) -> eyre::Result<()> {
        match action {
            LspAction::OpenDocument(path) => self.lsp.open_document(path),
            LspAction::Request {
                operation,
                mut query,
            } => {
                if query.path.is_relative() {
                    query.path = self.repository_root.join(&query.path);
                }
                self.lsp.request(operation, query)
            }
            LspAction::Restart => self.lsp.restart(),
        }
        .map_err(eyre::Report::msg)
    }

    fn settings(&mut self, action: SettingsAction) -> eyre::Result<()> {
        match action {
            SettingsAction::SaveFilePaneWidth(columns) => {
                self.settings.save_file_pane_width(columns)?;
            }
            SettingsAction::SaveEditorKeymap(keymap) => self.settings.save_editor_keymap(keymap)?,
        }
        Ok(())
    }

    fn repository(&mut self, action: RepositoryAction) -> eyre::Result<()> {
        self.commands.send(WorkerCommand::Repository(action))?;
        Ok(())
    }

    fn terminal(&mut self, action: TerminalAction) -> eyre::Result<ControlFlow<()>> {
        match action {
            TerminalAction::OpenInEditor { path, line } => {
                (self.open_in_editor)(&path, line)?;
                Ok(ControlFlow::Continue(()))
            }
            TerminalAction::Quit => Ok(ControlFlow::Break(())),
        }
    }
}

impl<B: CursorBackend> RuntimeEventLoop<'_, B>
where
    B::Error: Send + Sync + 'static,
{
    fn run(&mut self) -> eyre::Result<()> {
        while !self.cycle()? {}
        Ok(())
    }

    fn cycle(&mut self) -> eyre::Result<bool> {
        let event = self
            .events
            .recv()
            .map_err(|_| eyre::eyre!("all review event producers stopped unexpectedly"))?;
        if self.is_idle_tick(&event) {
            return Ok(false);
        }
        let started = Instant::now();
        let mut redraw = self.event_needs_frame(&event);
        if self.handle_event(&event)? {
            return Ok(true);
        }
        for _ in 1..EVENT_BATCH_LIMIT {
            if started.elapsed() >= EVENT_BATCH_BUDGET {
                break;
            }
            let Some(event) = self.events.try_recv() else {
                break;
            };
            redraw |= self.event_needs_frame(&event);
            if self.handle_event(&event)? {
                return Ok(true);
            }
        }
        if redraw {
            self.dispatch_event(&EventEnvelope::new(ui_events::ExploreCoverageRefresh))?;
            self.redraw()?;
        }
        Ok(false)
    }

    fn is_idle_tick(&self, event: &EventEnvelope) -> bool {
        event
            .downcast_ref::<ApplicationTick>()
            .is_some_and(|ApplicationTick(now)| !self.app.needs_tick(self.last_frame, *now))
    }

    fn event_needs_frame(&self, event: &EventEnvelope) -> bool {
        // Agent detection events belong to the delivery worker. Its resulting UI
        // events redraw when needed; the detection notification itself changes no UI.
        !self.is_idle_tick(event)
            && !matches!(
                event.downcast_ref::<HerdrEvent>(),
                Some(HerdrEvent::AgentDetected { .. })
            )
    }

    fn handle_event(&mut self, event: &EventEnvelope) -> eyre::Result<bool> {
        if self.is_idle_tick(event) {
            return Ok(false);
        }
        let started = Instant::now();
        if event.downcast_ref::<UserInput>().is_some()
            || event.downcast_ref::<TerminalFocused>().is_some()
            || matches!(
                event.downcast_ref::<HerdrEvent>(),
                Some(HerdrEvent::PaneFocused(_))
            )
        {
            self.terminal.backend_mut().invalidate_cursor_visibility();
        }
        let result = self.dispatch_event(event);
        self.timings.event(event, started);
        result
    }

    fn dispatch_event(&mut self, event: &EventEnvelope) -> eyre::Result<bool> {
        match self.handle_control_event(event)? {
            ControlEventOutcome::NotHandled => {}
            ControlEventOutcome::Continue => return Ok(false),
            ControlEventOutcome::Stop => return Ok(true),
        }
        let actions = self.application_actions(event);
        let mut dispatcher = RuntimeActionDispatcher {
            source_watches: self.source_watches,
            comments: self.comments,
            commands: self.commands,
            documents: self.documents,
            search: self.search,
            highlighting: self.highlighting,
            settings: self.settings,
            repository_root: self.repository_root,
            lsp: self.lsp,
            open_in_editor: &mut |path, line| self.open_in_editor(path, line),
        };
        Ok(dispatcher.run_all(actions)?.is_break())
    }

    fn open_in_editor(&mut self, path: &Path, line: Option<u32>) -> eyre::Result<()> {
        let Some(terminal_events) = self.terminal_events.as_deref_mut() else {
            return Ok(());
        };
        terminal_events.suspend();
        let result = editor::open(path, line)?;
        terminal_events.resume();
        self.terminal.clear()?;
        if let Err(error) = result {
            let _ = self.app.publish(ui_events::ToastRequested {
                text: format!("Could not open {}: {error}", path.display()),
                kind: toasts::ToastKind::Error,
            });
        }
        Ok(())
    }

    fn handle_control_event(&mut self, event: &EventEnvelope) -> eyre::Result<ControlEventOutcome> {
        if let Some(HerdrEvent::PaneFocused(pane_id)) = event.downcast_ref::<HerdrEvent>() {
            self.target.observe_focus(pane_id);
            self.comments.send(comments::Command::ActiveAgentChanged);
            return Ok(ControlEventOutcome::Continue);
        }
        if let Some(event) = event.downcast_ref::<HerdrEvent>() {
            self.comments
                .send(comments::Command::Observe(event.clone()));
            return Ok(ControlEventOutcome::Continue);
        }
        if event.downcast_ref::<WorkerStopped>().is_some() {
            eyre::bail!("review worker stopped unexpectedly");
        }
        if let Some(TerminalFailed(message)) = event.downcast_ref::<TerminalFailed>() {
            eyre::bail!("could not read terminal input: {message}");
        }
        if event.downcast_ref::<StopRequested>().is_some() {
            return Ok(ControlEventOutcome::Stop);
        }
        if event.downcast_ref::<RepositoryRefreshDue>().is_some() {
            self.commands.send(WorkerCommand::Poll)?;
            return Ok(ControlEventOutcome::NotHandled);
        }
        Ok(ControlEventOutcome::NotHandled)
    }

    fn application_actions(&mut self, event: &EventEnvelope) -> Vec<Action> {
        if let Some(input) = event.downcast_ref::<UserInput>() {
            self.app.update(input.clone())
        } else if event.downcast_ref::<RepositoryRefreshDue>().is_some() {
            self.app.publish(RepositoryRefreshStarted)
        } else if let Some(event) = event.downcast_ref::<review_lsp::Event>() {
            self.app.publish(event.clone())
        } else if let Some(ApplicationTick(now)) = event.downcast_ref::<ApplicationTick>() {
            let mut actions = self.app.publish(AnimationTick);
            actions.extend(self.app.publish(ToastExpirationTick { now: *now }));
            actions
        } else {
            self.app.publish_envelope(event)
        }
    }

    fn redraw(&mut self) -> eyre::Result<()> {
        let started = Instant::now();
        let area = self.terminal.size()?;
        let _ = self.app.update(UserInput::Resize {
            width: area.width,
            height: area.height,
        });
        self.terminal
            .draw(|frame| frame.render_widget(self.app.frame(), frame.area()))?;
        self.last_frame = started;
        self.timings.frame(started);
        self.dispatch_event(&EventEnvelope::new(ui_events::FrameRendered))?;
        Ok(())
    }
}

impl Worker {
    fn run(&mut self, commands: &Receiver<WorkerCommand>, messages: &ApplicationEventSender) {
        let mut next = None;
        while let Some(mut command) = next.take().or_else(|| commands.recv().ok()) {
            if let WorkerCommand::Explore(explore_session::Input::Command(
                review_explore::Command::SaveView(view),
            )) = &mut command
            {
                for _ in 0..64 {
                    match commands.try_recv() {
                        Ok(WorkerCommand::Explore(explore_session::Input::Command(
                            review_explore::Command::SaveView(new),
                        ))) if new.instance == view.instance => {
                            *view = new;
                        }
                        Ok(command) => {
                            next = Some(command);
                            break;
                        }
                        Err(_) => break,
                    }
                }
            }
            if !self.handle_command(command, messages) {
                return;
            }
        }
    }

    fn handle_command(
        &mut self,
        command: WorkerCommand,
        messages: &ApplicationEventSender,
    ) -> bool {
        match command {
            WorkerCommand::Poll => {
                let _ = self.poll(messages);
                let _ = messages.send(RepositoryRefreshFinished);
            }
            WorkerCommand::Repository(action) => self.handle_repository_action(action, messages),
            WorkerCommand::AutoReviewFinished(review) => self.finish_auto_review(&review, messages),
            WorkerCommand::Explore(input) => self.explore.handle(input),
            WorkerCommand::Quit => return false,
        }
        true
    }

    fn handle_repository_action(
        &mut self,
        action: RepositoryAction,
        messages: &ApplicationEventSender,
    ) {
        match action {
            RepositoryAction::LoadRevisionCandidates(direction) => {
                let result = self
                    .repository
                    .revision_candidates(direction)
                    .map_err(|error| error.to_string());
                let _ = messages.send(RevisionCandidatesLoaded { direction, result });
            }
            RepositoryAction::LoadRevisionHistory { load_id } => {
                let result = self
                    .repository
                    .revision_history()
                    .map_err(|error| error.to_string());
                let _ = messages.send(RevisionHistoryLoaded { load_id, result });
            }
            RepositoryAction::EditRevision { change_id } => {
                self.edit_revision(messages, &change_id);
            }
            RepositoryAction::SetReviewed { path, reviewed } => {
                self.cancel_auto_review();
                self.set_reviewed(messages, path, reviewed);
            }
            RepositoryAction::AutoReview(checkpoint) => {
                self.start_auto_review(&checkpoint, messages);
            }
            RepositoryAction::UnreviewAll(checkpoint) => self.unreview_all(&checkpoint, messages),
        }
    }

    fn edit_revision(&mut self, messages: &ApplicationEventSender, change_id: &ChangeId) {
        let failure = match self.repository.edit_revision(change_id) {
            Ok(true) if !self.poll(messages) => {
                Some("could not load the selected revision".to_owned())
            }
            Ok(true) => return,
            Ok(false) => Some("selected revision is immutable or unavailable".to_owned()),
            Err(error) => Some(error.to_string()),
        };
        let _ = messages.send(RevisionEditFailed { message: failure });
    }

    fn poll(&mut self, messages: &ApplicationEventSender) -> bool {
        let snapshot = match self.repository.poll() {
            Ok(PollResult::Complete(snapshot)) => snapshot,
            Ok(PollResult::ChangedDuringPoll) | Err(_) => return false,
        };
        let Ok(states) = self.tracker.statuses(&snapshot) else {
            return false;
        };
        let files = snapshot
            .files
            .iter()
            .zip(&states)
            .map(|(file, state)| FileSummary::from_review_state(file, *state))
            .collect();
        let review_checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_unit().clone(),
            snapshot.identity.snapshot_id(),
        );
        if self
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.identity != snapshot.identity)
        {
            self.cancel_auto_review();
        }
        let _ = self
            .documents
            .send(document::Command::Snapshot(snapshot.clone()));
        let _ = messages.send(RepositoryMetadataChanged {
            review_checkpoint: review_checkpoint.clone(),
            description: snapshot.identity.description().to_owned(),
            display_id: snapshot.identity.display_id().to_owned(),
        });
        let _ = messages.send(RepositoryFilesChanged {
            review_checkpoint,
            files,
        });
        let review_unit = snapshot.identity.review_unit().clone();
        self.explore.checkpoint_changed(&review_unit);
        self.snapshot = Some(snapshot);
        true
    }

    fn set_reviewed(&self, messages: &ApplicationEventSender, path: String, reviewed: bool) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let review_unit = snapshot.identity.review_unit().clone();
        let result = snapshot
            .files
            .iter()
            .find(|file| file.review_path().display() == path)
            .ok_or_else(|| eyre::eyre!("the selected file is no longer in the current change"))
            .and_then(|file| {
                if reviewed {
                    match self.tracker.mark(snapshot, file)? {
                        MarkResult::Marked => self.tracker.status(snapshot, file),
                        MarkResult::ChangeChanged => {
                            eyre::bail!("the change moved; wait for the next refresh");
                        }
                    }
                } else {
                    self.tracker.unreview(snapshot, file)?;
                    self.tracker.status(snapshot, file)
                }
            })
            .map_err(|_| ());
        let _ = messages.send(ReviewStateSaved {
            review_unit,
            path,
            result,
        });
    }
}

impl TerminalGuard {
    fn new() -> eyre::Result<Self> {
        enable_raw_mode()?;
        let mut backend = TerminalBackend::new(stdout());
        if env::var_os("HERDR_REVIEWER_VISION").is_some() {
            backend = backend.with_frame_capture();
        }
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = disable_raw_mode();
                return Err(error.into());
            }
        };
        if let Err(error) = enter_terminal_modes(terminal.backend_mut()) {
            let _ = disable_raw_mode();
            let _ = leave_terminal_modes(terminal.backend_mut());
            return Err(error.into());
        }
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = leave_terminal_modes(self.terminal.backend_mut());
        let _ = self.terminal.show_cursor();
    }
}

fn enter_terminal_modes(writer: &mut impl io::Write) -> io::Result<()> {
    execute!(
        writer,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableFocusChange,
        crossterm::event::EnableBracketedPaste,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )
}

fn leave_terminal_modes(writer: &mut impl io::Write) -> io::Result<()> {
    execute!(
        writer,
        DisableMouseCapture,
        DisableFocusChange,
        crossterm::event::DisableBracketedPaste,
        PopKeyboardEnhancementFlags,
        LeaveAlternateScreen
    )
}

fn normalize_mouse(mouse: MouseEvent) -> Option<UserInput> {
    let (column, row) = (mouse.column, mouse.row);
    let step = if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        6
    } else {
        3
    };
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(UserInput::MouseScroll {
            column,
            row,
            delta: -step,
        }),
        MouseEventKind::ScrollDown => Some(UserInput::MouseScroll {
            column,
            row,
            delta: step,
        }),
        MouseEventKind::Down(MouseButton::Left)
            if mouse.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            Some(UserInput::MouseControlClick { column, row })
        }
        MouseEventKind::Down(MouseButton::Left) => Some(UserInput::MouseClick { column, row }),
        MouseEventKind::Down(MouseButton::Right) => {
            Some(UserInput::MouseRightClick { column, row })
        }
        MouseEventKind::Drag(MouseButton::Left) => Some(UserInput::MouseDrag { column, row }),
        MouseEventKind::Up(MouseButton::Left) => Some(UserInput::MouseRelease),
        _ => None,
    }
}

impl TerminalEventProducer {
    fn start(events: EventSender<EventEnvelope>) -> Self {
        Self::start_with_reader(events, |timeout| {
            event::poll(timeout)?.then(event::read).transpose()
        })
    }

    fn start_with_reader(
        events: EventSender<EventEnvelope>,
        mut read_event: impl FnMut(Duration) -> io::Result<Option<Event>> + Send + 'static,
    ) -> Self {
        let stop_requested = Arc::new(AtomicBool::new(false));
        let reader_stop_requested = Arc::clone(&stop_requested);
        let sender = events.clone();
        let thread = thread::spawn(move || {
            let mut mouse_clicks = MouseClicks::default();
            while !reader_stop_requested.load(Ordering::Relaxed) {
                let event = match read_event(TIMER_INTERVAL) {
                    Ok(Some(event)) => event,
                    Ok(None) => continue,
                    Err(error) => {
                        let _ = events.send(EventEnvelope::new(TerminalFailed(error.to_string())));
                        return;
                    }
                };
                let message = Self::normalize_event(event, &mut mouse_clicks);
                if message.is_some_and(|message| events.send(message).is_err()) {
                    return;
                }
            }
        });
        Self {
            events: sender,
            stop_requested,
            thread: Some(thread),
        }
    }

    /// Stop reading so a child process owns terminal input.
    fn suspend(&mut self) {
        self.stop_and_join();
    }

    fn resume(&mut self) {
        *self = Self::start(self.events.clone());
    }

    fn normalize_event(event: Event, mouse_clicks: &mut MouseClicks) -> Option<EventEnvelope> {
        match event {
            Event::Paste(text) => Some(EventEnvelope::new(UserInput::Paste(text))),
            Event::Key(key) => Key::from_terminal(key)
                .map(UserInput::Key)
                .map(EventEnvelope::new),
            Event::Mouse(mouse) => mouse_clicks.normalize(mouse).map(EventEnvelope::new),
            Event::Resize(width, height) => {
                Some(EventEnvelope::new(UserInput::Resize { width, height }))
            }
            Event::FocusGained => Some(EventEnvelope::new(TerminalFocused)),
            Event::FocusLost => None,
        }
    }

    fn stop(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop_requested.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl RuntimeEventProducers {
    fn new(stop_requested: Arc<AtomicBool>) -> Self {
        Self {
            stop_requested,
            threads: Vec::new(),
        }
    }

    fn push(&mut self, thread: JoinHandle<()>) {
        self.threads.push(thread);
    }

    fn stop(mut self) {
        self.stop_and_join();
    }

    fn stop_and_join(&mut self) {
        self.stop_requested.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl Drop for RuntimeEventProducers {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

impl Drop for TerminalEventProducer {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

impl MouseClicks {
    fn normalize(&mut self, mouse: MouseEvent) -> Option<UserInput> {
        self.normalize_at(mouse, Instant::now())
    }

    fn normalize_at(&mut self, mouse: MouseEvent, now: Instant) -> Option<UserInput> {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.modifiers.is_empty() {
            let double = self.previous.take().is_some_and(|(column, row, previous)| {
                column.abs_diff(mouse.column) <= 1
                    && row == mouse.row
                    && now.saturating_duration_since(previous) <= DOUBLE_CLICK_INTERVAL
            });
            if double {
                return Some(UserInput::MouseDoubleClick {
                    column: mouse.column,
                    row: mouse.row,
                });
            }
            self.previous = Some((mouse.column, mouse.row, now));
        } else if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.previous = None;
        }
        normalize_mouse(mouse)
    }
}

#[cfg(test)]
#[path = "runtime.tests.rs"]
mod tests;
