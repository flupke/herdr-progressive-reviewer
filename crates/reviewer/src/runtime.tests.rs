use super::*;
use std::fs::{self, File};
use std::io::Write;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

use effects::fixture::EffectsFixture;
use herdr_client::protocol::{AgentPort, HerdrEvent};
use ratatui::backend::TestBackend;
use review_repository::diff::DiffRow;
use review_repository::repository::RepoType;
use review_source::ReviewCheckpoint;
use review_state::ReviewStatus;
use review_test_support::{HerdrTestServer, repository_fixture};
use review_types::ReviewUnit;
use review_ui::{DocumentAction, DocumentLoad};
use ui_events::{
    DiffContentLoaded, FileSummary, RepositoryFilesChanged, RepositoryRefreshFinished,
};

/// How long a test waits for Herdr, a worker thread or the repository to reach
/// a state. Every wait ends as soon as the state shows, so a long bound only
/// costs time when a test is about to fail; a short one fails under a full
/// parallel run.
const HERDR_WAIT: Duration = Duration::from_secs(30);

#[path = "runtime/mcp.tests.rs"]
mod mcp;

#[path = "runtime/agent_input.tests.rs"]
mod agent_input;
#[path = "runtime/explore.tests.rs"]
mod explore_flow;

const AGENT_E2E_AGENT_SOURCE: &str = "progressive-reviewer-e2e";

#[derive(Clone, Copy)]
enum AgentLifecycle {
    Reported,
    Native,
}

struct IsolatedHerdrServer {
    server: HerdrTestServer,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    agent_binary: PathBuf,
    agent: String,
}

impl IsolatedHerdrServer {
    fn start(repository_root: &std::path::Path) -> Self {
        Self::start_as(repository_root, "codex")
    }

    fn start_as(repository_root: &std::path::Path, agent: &str) -> Self {
        Self::start_with_session(repository_root, agent, Some("session"))
    }

    fn start_native(repository_root: &Path) -> Self {
        Self::start_with_lifecycle(repository_root, "codex", None, AgentLifecycle::Native)
    }

    fn start_with_session(repository_root: &Path, agent: &str, session: Option<&str>) -> Self {
        Self::start_with_lifecycle(repository_root, agent, session, AgentLifecycle::Reported)
    }

    fn start_with_lifecycle(
        repository_root: &Path,
        agent: &str,
        session: Option<&str>,
        lifecycle: AgentLifecycle,
    ) -> Self {
        let server = HerdrTestServer::start(repository_root);
        let prompt_path = server.root().join("prompt.txt");
        let prompt_environment = format!("REVIEW_AGENT_E2E_PROMPT_PATH={}", prompt_path.display());
        let binary_environment =
            format!("REVIEW_AGENT_E2E_HERDR_BIN={}", server.binary().display());
        let agent_environment = format!("REVIEW_AGENT_E2E_AGENT={agent}");
        let session_environment = format!(
            "REVIEW_AGENT_E2E_AGENT_SESSION={}",
            session.unwrap_or_default()
        );
        let report_environment = format!(
            "REVIEW_AGENT_E2E_REPORT_LIFECYCLE={}",
            match lifecycle {
                AgentLifecycle::Reported => "1",
                AgentLifecycle::Native => "0",
            }
        );
        let workspace = server.run_cli_json(&[
            "workspace",
            "create",
            "--cwd",
            &repository_root.to_string_lossy(),
            "--label",
            "review-source-e2e",
            "--env",
            &prompt_environment,
            "--env",
            &binary_environment,
            "--env",
            &agent_environment,
            "--env",
            &session_environment,
            "--env",
            &report_environment,
            "--no-focus",
        ]);
        let workspace_id = WorkspaceId(
            workspace["result"]["workspace"]["workspace_id"]
                .as_str()
                .expect("workspace create must return a workspace ID")
                .to_owned(),
        );
        let pane_id = PaneId(
            workspace["result"]["root_pane"]["pane_id"]
                .as_str()
                .expect("workspace create must return a root pane ID")
                .to_owned(),
        );
        let current_test_binary = std::env::current_exe().unwrap();
        let agent_binary = server.root().join(agent);
        fs::copy(current_test_binary, &agent_binary).unwrap();
        let server = Self {
            server,
            workspace_id,
            pane_id,
            agent_binary,
            agent: agent.into(),
        };
        server.start_agent();
        server.wait_for_agent(session);
        server
    }

    fn start_agent(&self) {
        self.run_cli(&[
            "pane",
            "run",
            &self.pane_id.0,
            &self.agent_binary.to_string_lossy(),
            "--exact",
            "runtime::tests::e2e_agent_process",
            "--nocapture",
        ]);
    }

    fn stop_agent(&self) {
        // Native detection must observe the exit; release-agent would reset it
        // and could leave a stale agent record after the process is gone.
        self.run_cli(&["pane", "send-keys", &self.pane_id.0, "ctrl+d"]);
        let deadline = Instant::now() + HERDR_WAIT;
        while self.client().get_agent(&self.pane_id).unwrap().is_some()
            || self
                .client()
                .pane_process_info(&self.pane_id)
                .unwrap()
                .foreground_processes
                .iter()
                .any(|process| process.name == self.agent)
        {
            assert!(
                Instant::now() < deadline,
                "the previous test agent did not exit"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn client(&self) -> HerdrClient {
        self.server.client()
    }

    fn wait_for_agent(&self, session: Option<&str>) {
        let client = self.client();
        let deadline = Instant::now() + HERDR_WAIT;
        while Instant::now() < deadline {
            if client.get_agent(&self.pane_id).is_ok_and(|agent| {
                agent.is_some_and(|agent| {
                    agent
                        .agent_session
                        .as_ref()
                        .map(|session| session.value.as_str())
                        == session
                })
            }) {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "expected agent session {session:?}, got {:?}",
            client.get_agent(&self.pane_id)
        );
    }

    fn run_cli(&self, arguments: &[&str]) -> std::process::Output {
        self.server.run_cli(arguments)
    }

    fn report_agent(&self, state: &str) {
        self.run_cli(&[
            "pane",
            "report-agent",
            &self.pane_id.0,
            "--source",
            AGENT_E2E_AGENT_SOURCE,
            "--agent",
            &self.agent,
            "--state",
            state,
        ]);
    }

    fn release_agent(&self) {
        self.run_cli(&[
            "pane",
            "release-agent",
            &self.pane_id.0,
            "--source",
            AGENT_E2E_AGENT_SOURCE,
            "--agent",
            &self.agent,
        ]);
    }

    fn report_session(&self, session: &str) {
        self.run_cli(&[
            "pane",
            "report-agent-session",
            &self.pane_id.0,
            "--source",
            &format!("herdr:{}", self.agent),
            "--agent",
            &self.agent,
            "--agent-session-id",
            session,
        ]);
        self.wait_for_agent(Some(session));
    }
}

#[test]
fn e2e_agent_process() {
    let Some(prompt_path) = std::env::var_os("REVIEW_AGENT_E2E_PROMPT_PATH") else {
        return;
    };
    // Match a native TUI: the terminal must not submit pasted newlines, echo
    // input, or truncate long lines through its canonical input buffer.
    crossterm::terminal::enable_raw_mode().unwrap();
    crossterm::execute!(io::stdout(), crossterm::event::EnableBracketedPaste).unwrap();
    let binary = std::env::var_os("REVIEW_AGENT_E2E_HERDR_BIN").unwrap();
    let pane_id = std::env::var("HERDR_PANE_ID").unwrap();
    let agent = std::env::var("REVIEW_AGENT_E2E_AGENT").unwrap();
    if std::env::var("REVIEW_AGENT_E2E_REPORT_LIFECYCLE").as_deref() != Ok("0") {
        let status = Command::new(&binary)
            .args([
                "pane",
                "report-agent",
                &pane_id,
                "--source",
                AGENT_E2E_AGENT_SOURCE,
                "--agent",
                &agent,
                "--state",
                "idle",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    if let Ok(session) = std::env::var("REVIEW_AGENT_E2E_AGENT_SESSION")
        && !session.is_empty()
    {
        let status = Command::new(&binary)
            .args([
                "pane",
                "report-agent-session",
                &pane_id,
                "--source",
                &format!("herdr:{agent}"),
                "--agent",
                &agent,
                "--agent-session-id",
                &session,
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    let state_path = PathBuf::from(&prompt_path).with_extension("state");
    let screen_path = PathBuf::from(&prompt_path).with_extension("screen");
    let screen_updates = screen_path.clone();
    thread::spawn(move || {
        let mut previous = String::new();
        let mut previous_screen = String::new();
        loop {
            if let Ok(title) = fs::read_to_string(&state_path)
                && title != previous
            {
                print!("\x1b]0;{title}\x07");
                io::stdout().flush().unwrap();
                previous = title;
            }
            if let Ok(screen) = fs::read_to_string(&screen_updates)
                && screen != previous_screen
            {
                print!("\x1b[2J\x1b[H{}", screen.replace('\n', "\r\n"));
                io::stdout().flush().unwrap();
                previous_screen = screen;
            }
            thread::sleep(Duration::from_millis(25));
        }
    });
    let mut prompt_file = File::options()
        .create(true)
        .append(true)
        .open(prompt_path)
        .unwrap();
    let marker = if agent == "claude" { "❯" } else { "›" };
    print!("\x1b[2J\x1b[H{marker} ");
    io::stdout().flush().unwrap();
    let mut input = agent_input::AgentInput::default();
    loop {
        let event = crossterm::event::read().unwrap();
        if matches!(event, crossterm::event::Event::Key(key)
            if key.code == crossterm::event::KeyCode::Char('d')
                && key.modifiers == crossterm::event::KeyModifiers::CONTROL)
        {
            crossterm::terminal::disable_raw_mode().unwrap();
            crossterm::execute!(io::stdout(), crossterm::event::DisableBracketedPaste).unwrap();
            return;
        }
        if let Some(prompt) = input.handle(event) {
            writeln!(prompt_file, "{prompt}").unwrap();
            prompt_file.flush().unwrap();
        }
        let screen = if input.text().is_empty() {
            fs::read_to_string(&screen_path).unwrap_or_else(|_| format!("{marker} "))
        } else {
            format!("{marker} {}", input.text())
        };
        print!("\x1b[2J\x1b[H{}", screen.replace('\n', "\r\n"));
        io::stdout().flush().unwrap();
    }
}

fn receive_agent_release(events: &Receiver<HerdrEvent>) {
    loop {
        let event = events.recv_timeout(HERDR_WAIT).unwrap();
        if matches!(event, HerdrEvent::AgentDetected { released: true, .. }) {
            return;
        }
    }
}

struct AgentEventSubscription {
    events: Receiver<HerdrEvent>,
    continue_streaming: Arc<AtomicBool>,
    thread: JoinHandle<herdr_client::Result<()>>,
}

impl AgentEventSubscription {
    fn start(herdr: &IsolatedHerdrServer) -> Self {
        let client = herdr.client();
        let continue_streaming = Arc::new(AtomicBool::new(true));
        let thread_continue_streaming = Arc::clone(&continue_streaming);
        let (event_sender, events) = mpsc::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let mut ready_sender = Some(ready_sender);
            client.forward_events_while(
                || {
                    // The cancellation callback first runs after subscription acknowledgement.
                    if let Some(sender) = ready_sender.take() {
                        sender.send(()).unwrap();
                    }
                    thread_continue_streaming.load(Ordering::Relaxed)
                },
                |event| event_sender.send(event).is_ok(),
            )
        });
        ready.recv_timeout(HERDR_WAIT).unwrap();
        Self {
            events,
            continue_streaming,
            thread,
        }
    }
}

fn confirm_multiple_event_subscribers(herdr: &IsolatedHerdrServer) {
    let first = AgentEventSubscription::start(herdr);
    let second = AgentEventSubscription::start(herdr);

    herdr.release_agent();
    receive_agent_release(&first.events);
    receive_agent_release(&second.events);
    drop(first.events);
    drop(second.events);
    herdr.report_agent("idle");
    first.thread.join().unwrap().unwrap();
    second.thread.join().unwrap().unwrap();
}

#[test]
fn simultaneous_herdr_event_subscribers_stay_connected() {
    let repository = tempfile::tempdir().unwrap();
    let herdr = IsolatedHerdrServer::start(repository.path());

    confirm_multiple_event_subscribers(&herdr);
}

#[test]
fn herdr_event_subscription_stops_without_a_new_server_event() {
    let repository = tempfile::tempdir().unwrap();
    let herdr = IsolatedHerdrServer::start(repository.path());
    let subscription = AgentEventSubscription::start(&herdr);

    subscription
        .continue_streaming
        .store(false, Ordering::Relaxed);

    subscription.thread.join().unwrap().unwrap();
}

struct ReviewFlowFixture {
    /// Declared first so the workers stop before the Herdr server.
    runtime: EffectsFixture,
    herdr: IsolatedHerdrServer,
    review_unit: ReviewUnit,
    endpoint: review_mcp::Endpoint,
    _port: review_test_support::TestPort,
}

impl ReviewFlowFixture {
    fn start(repository_type: RepoType) -> Self {
        let repository_files = repository_fixture(repository_type);
        repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let herdr = IsolatedHerdrServer::start_native(repository_files.root());
        let port = review_test_support::TestPort::new();
        let endpoint =
            review_mcp::Endpoint::for_repository(repository_files.root(), Some(port.number()))
                .unwrap();
        herdr.report_agent("idle");
        herdr.run_cli(&[
            "pane",
            "split",
            &herdr.pane_id.0,
            "--direction",
            "right",
            "--no-focus",
        ]);
        herdr.run_cli(&[
            "pane",
            "focus",
            "--direction",
            "right",
            "--pane",
            &herdr.pane_id.0,
        ]);
        let mut runtime = EffectsFixture::start(repository_files, |setup| {
            setup.agents = herdr.client();
            setup.target =
                AgentTarget::new(herdr.workspace_id.clone(), Some(herdr.pane_id.clone()));
            setup.endpoint = Ok(endpoint);
        });
        let review_unit = runtime.refreshed_checkpoint().review_unit;
        Self {
            runtime,
            herdr,
            review_unit,
            endpoint,
            _port: port,
        }
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn disk_content_changes_replace_the_visible_diff(repository_type: RepoType) {
    let repository_files = repository_fixture(repository_type);
    repository_files.write("src/lib.rs", b"pub fn before_refresh() {}\n");
    let mut fixture = EffectsFixture::start(repository_files, |_| {});
    let mut application = ReviewApplication::default();

    let initial_actions = publish_refresh(&mut fixture, &mut application);
    load_requested_diffs(&mut fixture, &mut application, initial_actions);
    assert!(rendered_review_application(&application).contains("before_refresh"));

    fixture
        .files
        .write("src/lib.rs", b"pub fn after_refresh() {}\n");
    let refresh_actions = publish_refresh(&mut fixture, &mut application);
    assert!(matches!(
        refresh_actions.as_slice(),
        [Action::Document(DocumentAction::Load(
            DocumentLoad::Diff { .. }
        ))]
    ));
    load_requested_diffs(&mut fixture, &mut application, refresh_actions);

    let rendered = rendered_review_application(&application);
    assert!(rendered.contains("after_refresh"), "{rendered}");
    assert!(!rendered.contains("before_refresh"), "{rendered}");
}

/// Publish a refresh's events and return the document loads they request.
fn publish_refresh(
    fixture: &mut EffectsFixture,
    application: &mut ReviewApplication,
) -> Vec<Action> {
    fixture
        .refresh()
        .iter()
        .flat_map(|event| application.publish_envelope(event))
        .filter(|action| matches!(action, Action::Document(DocumentAction::Load(_))))
        .collect()
}

/// Perform the loads and publish events until the last diff arrives.
fn load_requested_diffs(
    fixture: &mut EffectsFixture,
    application: &mut ReviewApplication,
    actions: Vec<Action>,
) {
    let mut pending = actions.len();
    fixture.perform(actions);
    while pending > 0 {
        let event = fixture.next_event();
        if event.downcast_ref::<DiffContentLoaded>().is_some() {
            pending -= 1;
        }
        application.publish_envelope(&event);
    }
}

fn rendered_review_application(application: &ReviewApplication) -> String {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(application.frame(), frame.area()))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn references_are_restricted_to_the_rust_project() {
    let location = |path| review_lsp::SourceLocation {
        path: PathBuf::from(path),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 1,
    };
    let locations = vec![
        location("/repo/crates/src/lib.rs"),
        location("/repo/src/lib.rs"),
        location("/dependency/src/lib.rs"),
    ];

    assert_eq!(
        review_lsp::Operation::References
            .filter_locations(std::path::Path::new("/repo/crates"), locations)
            .into_iter()
            .map(|location| location.path)
            .collect::<Vec<_>>(),
        vec![PathBuf::from("/repo/crates/src/lib.rs")]
    );
}

#[test]
fn modified_mouse_inputs_reuse_existing_actions() {
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row: 5,
            modifiers: KeyModifiers::SHIFT,
        }),
        Some(UserInput::MouseScroll {
            column: 4,
            row: 5,
            delta: 6,
        })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(UserInput::MouseScroll {
            column: 4,
            row: 5,
            delta: 3,
        })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 5,
            modifiers: KeyModifiers::SHIFT,
        }),
        Some(UserInput::MouseClick { column: 4, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Middle),
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        None
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 5,
            modifiers: KeyModifiers::CONTROL,
        }),
        Some(UserInput::MouseControlClick { column: 4, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(UserInput::MouseDrag { column: 40, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(UserInput::MouseRelease)
    );
}

#[test]
fn consecutive_plain_clicks_at_one_position_become_a_double_click() {
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: 5,
        modifiers: KeyModifiers::NONE,
    };
    let start = Instant::now();
    let mut clicks = MouseClicks::default();

    assert!(matches!(
        clicks.normalize_at(click, start),
        Some(UserInput::MouseClick { .. })
    ));
    let adjacent = MouseEvent { column: 5, ..click };
    assert_eq!(
        clicks.normalize_at(adjacent, start + Duration::from_millis(400)),
        Some(UserInput::MouseDoubleClick { column: 5, row: 5 })
    );
    assert!(matches!(
        clicks.normalize_at(click, start + Duration::from_millis(450)),
        Some(UserInput::MouseClick { .. })
    ));
    let modified = MouseEvent {
        modifiers: KeyModifiers::CONTROL,
        ..click
    };
    clicks.normalize_at(modified, start + Duration::from_millis(460));
    assert!(matches!(
        clicks.normalize_at(click, start + Duration::from_millis(470)),
        Some(UserInput::MouseClick { .. })
    ));
}

#[test]
fn event_loop_routes_external_events_from_the_central_channel() {
    let mut fixture = EffectsFixture::new(RepoType::Git);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut app = ReviewApplication::default();
    let focused_pane = PaneId("focused-agent".to_owned());
    for event in [
        EventEnvelope::new(HerdrEvent::PaneFocused(focused_pane)),
        EventEnvelope::new(RepositoryRefreshDue),
        EventEnvelope::new(ApplicationTick(Instant::now())),
        EventEnvelope::new(StopRequested),
    ] {
        fixture.background.send(event).unwrap();
    }

    RuntimeEventLoop {
        effects: &mut fixture.effects,
        terminal_events: None,
        terminal: &mut terminal,
        app: &mut app,
        events: &mut fixture.inbox,
        timings: &timing::Recorder::default(),
        last_frame: Instant::now(),
    }
    .run()
    .unwrap();

    let refresh = fixture.events_until::<RepositoryRefreshFinished>();
    assert!(
        refresh
            .iter()
            .any(|event| event.downcast_ref::<RepositoryFilesChanged>().is_some())
    );
}

#[test]
fn runtime_event_producers_join_active_lsp_adapter() {
    let repository = tempfile::tempdir().unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (event_sender, events) = unbounded();
    let stop_requested = Arc::new(AtomicBool::new(false));
    let mut producers = RuntimeEventProducers::new(Arc::clone(&stop_requested));
    producers.push(Runtime::start_lsp_events(
        lsp.event_receiver(),
        event_sender.clone(),
        stop_requested,
    ));
    drop(event_sender);

    producers.stop();

    assert!(matches!(
        events.try_recv(),
        Err(crossbeam_channel::TryRecvError::Disconnected)
    ));
}

#[test]
fn terminal_event_producer_stops_while_waiting_for_input() {
    let (event_sender, events) = unbounded();
    let (reader_started_sender, reader_started_receiver) = mpsc::channel();
    let producer = TerminalEventProducer::start_with_reader(event_sender, move |timeout| {
        let _ = reader_started_sender.send(());
        thread::sleep(timeout);
        Ok(None)
    });
    reader_started_receiver
        .recv_timeout(HERDR_WAIT)
        .expect("the terminal reader must start");

    producer.stop();

    assert!(matches!(
        events.try_recv(),
        Err(crossbeam_channel::TryRecvError::Disconnected)
    ));
}

#[test]
fn terminal_hunk_shortcut_moves_application_data_while_files_are_focused() {
    let review_checkpoint = ReviewCheckpoint::new("change", "checkpoint");
    let mut application = ReviewApplication::default();
    application.update(UserInput::Resize {
        width: 80,
        height: 12,
    });
    application.publish(RepositoryFilesChanged {
        review_checkpoint: review_checkpoint.clone(),
        files: vec![FileSummary::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    application.publish(DiffContentLoaded {
        review_checkpoint,
        path: "src/lib.rs".to_owned(),
        rows: hunk_navigation_rows(),
        old_content: None,
        new_content: None,
        hunks: review_hunks::FileHunks::default(),
    });
    let terminal_events = std::sync::Mutex::new(
        [
            Event::Key(Key::Char(']').to_terminal()),
            Event::Key(Key::Char('h').to_terminal()),
            Event::Key(Key::Char('[').to_terminal()),
            Event::Key(Key::Char('h').to_terminal()),
            Event::Key(Key::Char(']').to_terminal()),
            Event::Key(Key::Char('h').to_terminal()),
        ]
        .into_iter(),
    );
    let (event_sender, events) = unbounded();
    let producer = TerminalEventProducer::start_with_reader(event_sender, move |_| {
        Ok(terminal_events.lock().unwrap().next())
    });

    for (expected_text, other_text) in [
        ("second change", "first change"),
        ("first change", "second change"),
        ("second change", "first change"),
    ] {
        for _ in 0..2 {
            let event = events.recv_timeout(HERDR_WAIT).unwrap();
            let input = event.downcast_ref::<UserInput>().unwrap().clone();
            application.update(input);
        }
        let mut terminal = ratatui::Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(application.frame(), frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let expected_cell = rendered_text_cell(buffer, expected_text);
        let other_cell = rendered_text_cell(buffer, other_text);
        assert_ne!(expected_cell.bg, other_cell.bg);
    }
    producer.stop();
}

fn rendered_text_cell<'a>(
    buffer: &'a ratatui::buffer::Buffer,
    text: &str,
) -> &'a ratatui::buffer::Cell {
    for row in buffer.area.y..buffer.area.bottom() {
        let rendered = (buffer.area.x..buffer.area.right())
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        if let Some(column) = rendered.find(text) {
            return &buffer[(u16::try_from(column).unwrap(), row)];
        }
    }
    panic!("rendered text not found: {text}");
}

fn hunk_navigation_rows() -> Vec<DiffRow> {
    vec![
        DiffRow::Hunk {
            old_start: 1,
            old_count: 0,
            new_start: 1,
            new_count: 1,
        },
        DiffRow::Add {
            new_line: 1,
            text: "+first change".to_owned(),
        },
        DiffRow::Hunk {
            old_start: 20,
            old_count: 0,
            new_start: 21,
            new_count: 1,
        },
        DiffRow::Add {
            new_line: 21,
            text: "+second change".to_owned(),
        },
    ]
}
