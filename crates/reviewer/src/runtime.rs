//! Terminal and worker integration for the review pane.

mod actions;
mod auto_review;
mod document;
mod editor;
mod effects;
mod events;
mod highlighting;
mod jev;
mod page_sharing;
mod page_threads;
mod review_marks;
mod route;
mod terminal;
mod timing;
mod worker;

#[cfg(all(test, unix))]
#[path = "runtime/responsiveness.tests.rs"]
mod responsiveness;

use std::env;
use std::io::{self, stdout};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use component_core::EventEnvelope;
use crossbeam_channel::{Receiver as EventReceiver, Sender as EventSender, unbounded};
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
use herdr_client::protocol::{AgentTarget, PaneId, PluginContext, WorkspaceId};
use ratatui::Terminal;
use ratatui::backend::Backend;
use review_explore_page::{
    CommandSender, PageConversation, PageRound, RoundFeed, RoundPublisher, ThreadsFeed,
};
use review_explore_page_host::{Browser, PageDirectory, PageHost, PageOpener};
use review_explore_page_settings::ExplorePageSettings;
use review_explore_session as explore_session;
use review_repository::repository::Repository;
use review_store::ReviewStore;
use review_ui::{Action, Key, ReviewApplication, TerminalAction, Theme, UserInput};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::flag;
use terminal::{CursorBackend, TerminalBackend};
use ui_events::{AnimationTick, RepositoryRefreshStarted, ToastExpirationTick};

use crate::watcher::RepositoryWatcher;
use effects::{Effects, Outputs, RunAheadSetup, Setup};
use route::{
    ApplicationTick, RepositoryRefreshDue, Route, StopRequested, TerminalFailed, TerminalFocused,
};

const TIMER_INTERVAL: Duration = Duration::from_millis(50);
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);
const HERDR_EVENT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const EVENT_BATCH_LIMIT: usize = 64;
const EVENT_BATCH_BUDGET: Duration = Duration::from_millis(8);

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
    effects: &'a mut Effects,
    terminal_events: Option<&'a mut TerminalEventProducer>,
    terminal: &'a mut Terminal<B>,
    app: &'a mut ReviewApplication,
    events: &'a mut events::Inbox,
    timings: &'a timing::Recorder,
    last_frame: Instant,
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
        let mut app = ReviewApplication::new(self.theme, file_pane_width, root);
        app.set_editor_keymap(settings.editor_keymap()?);
        let explore_page = settings.explore_page_settings()?;
        let _ = app.publish(ui_events::ExplorePageSettingsLoaded(explore_page.clone()));
        let _ = app.publish(ui_events::ExploreRoundSettingsLoaded(
            settings.explore_round_settings()?,
        ));
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
        let page_round = RoundPublisher::default();
        let page_stages = page_round.subscribe();
        let page_threads = Arc::new(page_threads::PageThreads::default());
        let watcher = RepositoryWatcher::new(self.repository.watch_plan());
        let mut effects = Effects::start(
            Setup {
                repository: self.repository.clone(),
                store: settings.clone(),
                target: AgentTarget::new(self.workspace_id.clone(), self.initial_agent.clone()),
                agents: self.client.clone(),
                endpoint: review_mcp::Endpoint::from_env(self.repository.root()),
                theme: self.theme,
                jev: jev::classifier_from_env(),
                turns: vision_turns_from_env(),
                source_watches: Some(watcher.source_requests()),
                page: page_round,
                page_opener: Some(self.page_opener()),
                page_threads: Arc::clone(&page_threads),
                run_ahead: RunAheadSetup {
                    tools: claude_fork::ForkTools::beside_current_exe()?,
                    log: Some(self.state_dir.join("run-ahead.log")),
                },
            },
            &Outputs {
                background: event_sender.clone(),
                interactive: input_sender.clone(),
            },
        );
        let page = self.serve_explore_page(
            page_stages,
            page_threads.subscribe(),
            &explore_page,
            &mut effects,
            &event_sender,
        );
        let producer_stop_requested = Arc::new(AtomicBool::new(false));
        let mut event_producers = RuntimeEventProducers::new(Arc::clone(&producer_stop_requested));
        event_producers.push(Self::start_herdr_events(
            self.client.clone(),
            event_sender.clone(),
            Arc::clone(&producer_stop_requested),
        ));
        let mut terminal_events = TerminalEventProducer::start(input_sender);
        Self::watch_explore_storage(&watcher, &settings, &effects, event_sender.clone());
        event_producers.push(Self::start_periodic_events(
            event_sender.clone(),
            Arc::clone(&stopped),
            Arc::clone(&producer_stop_requested),
            watcher,
        ));
        event_producers.push(Self::start_lsp_events(
            effects.lsp_events(),
            event_sender,
            producer_stop_requested,
        ));
        let _ = app.publish(RepositoryRefreshStarted);
        effects.refresh()?;
        let result = RuntimeEventLoop {
            effects: &mut effects,
            terminal_events: Some(&mut terminal_events),
            terminal: &mut terminal.terminal,
            app: &mut app,
            events: &mut events::Inbox::new(events, inputs),
            timings: &timings,
            last_frame: Instant::now(),
        }
        .run();
        terminal_events.stop();
        event_producers.stop();
        self.repository.cancel();
        drop(terminal);
        drop(page);
        drop(effects);
        result
    }

    /// Serve the Explore page of the session's round, for the Herdr action that opens it, and
    /// to the network for the pane's QR code as the `settings` say. The page's commands join
    /// the session's other inputs, and later settings reach the page through the `effects`.
    fn serve_explore_page(
        &self,
        stages: RoundFeed,
        threads: ThreadsFeed,
        settings: &ExplorePageSettings,
        effects: &mut Effects,
        events: &EventSender<EventEnvelope>,
    ) -> Option<PageHost> {
        let directory = PageDirectory::new(&self.state_dir);
        let explore = effects.explore_inbox();
        let commands = CommandSender::new(move |command, reply| {
            explore.deliver(explore_session::Input::Page { command, reply });
        });
        let conversation = PageConversation::new(threads, effects.page_thread_sender());
        let round = PageRound::new(stages, commands).with_conversation(conversation);
        let review = self.repository.root();
        let host = match PageHost::start(round, &directory, &self.workspace_id, review) {
            Ok(host) => host,
            Err(error) => {
                let _ = events.send(EventEnvelope::new(ui_events::ToastRequested {
                    text: format!("Cannot serve the Explore page: {error}"),
                    kind: toasts::ToastKind::Error,
                }));
                return None;
            }
        };
        let sharing = page_sharing::PageSharing::new(host.network(), events.clone());
        sharing.apply(&settings.network);
        effects.share_page(sharing);
        Some(host)
    }

    /// What opens the Explore page from the pane, as the Herdr action opens it.
    fn page_opener(&self) -> PageOpener {
        PageOpener::new(
            PageDirectory::new(&self.state_dir),
            self.workspace_id.clone(),
            Browser::from_env(),
        )
    }

    /// Saved Explore state changed by another reviewer reaches the Explore session.
    fn watch_explore_storage(
        watcher: &RepositoryWatcher,
        settings: &ReviewStore,
        effects: &Effects,
        events: EventSender<EventEnvelope>,
    ) {
        if let Err(error) = settings.prepare_explore_storage() {
            let _ = events.send(EventEnvelope::new(ui_events::ExploreStorageFailed(
                error.to_string(),
            )));
            return;
        }
        let explore = effects.explore_inbox();
        watcher.watch_explore(settings.explore_directory(), move |event| match event {
            Ok(()) => explore.deliver(explore_session::Input::StorageChanged),
            Err(error) => {
                let _ = events.send(EventEnvelope::new(ui_events::ExploreStorageFailed(
                    format!("Watch Explore state: {error}"),
                )));
            }
        });
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
        lsp_events: EventReceiver<review_lsp::Event>,
        events: EventSender<EventEnvelope>,
        stop_requested: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
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
        if self.is_idle(&Route::of(&event)) {
            return Ok(false);
        }
        let started = Instant::now();
        let mut redraw = false;
        let mut next = Some(event);
        let mut remaining = EVENT_BATCH_LIMIT;
        while let Some(event) = next {
            match self.handle_event(&event)? {
                ControlFlow::Break(()) => return Ok(true),
                ControlFlow::Continue(needs_frame) => redraw |= needs_frame,
            }
            remaining -= 1;
            next = if remaining > 0 && started.elapsed() < EVENT_BATCH_BUDGET {
                self.events.try_recv()
            } else {
                None
            };
        }
        if redraw {
            self.redraw()?;
        }
        Ok(false)
    }

    /// A tick with nothing to animate or expire.
    fn is_idle(&self, route: &Route<'_>) -> bool {
        matches!(route, Route::Tick(now) if !self.app.needs_tick(self.last_frame, *now))
    }

    /// Handle one event; `Continue` says whether the screen may have changed.
    fn handle_event(&mut self, event: &EventEnvelope) -> eyre::Result<ControlFlow<(), bool>> {
        let route = Route::of(event);
        if self.is_idle(&route) {
            return Ok(ControlFlow::Continue(false));
        }
        let started = Instant::now();
        if route.resets_cursor() {
            self.terminal.backend_mut().invalidate_cursor_visibility();
        }
        let needs_frame = route.needs_frame();
        let flow = self.dispatch(route, event);
        self.timings.event(event, started);
        Ok(match flow? {
            ControlFlow::Break(()) => ControlFlow::Break(()),
            ControlFlow::Continue(()) => ControlFlow::Continue(needs_frame),
        })
    }

    /// Handle one event without timing it; true stops the runtime.
    fn dispatch_event(&mut self, event: &EventEnvelope) -> eyre::Result<bool> {
        Ok(self.dispatch(Route::of(event), event)?.is_break())
    }

    fn dispatch(
        &mut self,
        route: Route<'_>,
        event: &EventEnvelope,
    ) -> eyre::Result<ControlFlow<()>> {
        let actions = match route {
            Route::Fail(message) => return Err(eyre::eyre!(message)),
            Route::Stop => return Ok(ControlFlow::Break(())),
            Route::AgentFocused(pane_id) => {
                self.effects.agent_focused(pane_id);
                return Ok(ControlFlow::Continue(()));
            }
            Route::Herdr(event) => {
                self.effects.observe(event);
                return Ok(ControlFlow::Continue(()));
            }
            Route::RefreshDue => {
                self.effects.refresh()?;
                self.app.publish(RepositoryRefreshStarted)
            }
            route => self.application_actions(&route, event),
        };
        self.perform(actions)
    }

    fn application_actions(&mut self, route: &Route<'_>, event: &EventEnvelope) -> Vec<Action> {
        match *route {
            Route::Input(input) => self.app.update(input.clone()),
            Route::Lsp(event) => self.app.publish(event.clone()),
            Route::Tick(now) => {
                let mut actions = self.app.publish(AnimationTick);
                actions.extend(self.app.publish(ToastExpirationTick { now }));
                actions
            }
            _ => self.app.publish_envelope(event),
        }
    }

    fn perform(&mut self, actions: Vec<Action>) -> eyre::Result<ControlFlow<()>> {
        let Self {
            effects,
            terminal,
            terminal_events,
            app,
            ..
        } = self;
        effects.perform_all(actions, &mut |action| match action {
            TerminalAction::OpenInEditor { path, line } => {
                open_in_editor(terminal, terminal_events.as_deref_mut(), app, &path, line)?;
                Ok(ControlFlow::Continue(()))
            }
            TerminalAction::Quit => Ok(ControlFlow::Break(())),
        })
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

/// Test tooling only: a `make vision` session names the directory its
/// scripted agent reads the sent Explore prompts from. Without
/// `HERDR_REVIEWER_VISION` the directory is ignored, because the turn files
/// hold access values.
fn vision_turns_from_env() -> Option<explore_session::TurnLog> {
    env::var_os("HERDR_REVIEWER_VISION")?;
    let directory = env::var_os("HERDR_REVIEWER_VISION_TURNS")?;
    explore_session::TurnLog::open(directory.into()).ok()
}

/// Hand the terminal to the user's editor, then take it back.
fn open_in_editor<B: CursorBackend>(
    terminal: &mut Terminal<B>,
    terminal_events: Option<&mut TerminalEventProducer>,
    app: &mut ReviewApplication,
    path: &Path,
    line: Option<u32>,
) -> eyre::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    let Some(terminal_events) = terminal_events else {
        return Ok(());
    };
    terminal_events.suspend();
    let result = editor::open(path, line)?;
    terminal_events.resume();
    terminal.clear()?;
    if let Err(error) = result {
        let _ = app.publish(ui_events::ToastRequested {
            text: format!("Could not open {}: {error}", path.display()),
            kind: toasts::ToastKind::Error,
        });
    }
    Ok(())
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
