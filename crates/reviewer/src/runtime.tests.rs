use super::*;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::Command;

use herdr_client::protocol::AgentPort;
use ratatui::backend::TestBackend;
use review_repository::diff::DiffRow;
use review_repository::repository::RepoType;
use review_state::ReviewStatus;
use review_test_support::{
    HerdrTestServer, ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use review_types::ReviewUnit;

#[path = "runtime/mcp.tests.rs"]
mod mcp;

#[path = "runtime/agent_input.tests.rs"]
mod agent_input;
#[path = "runtime/explore.tests.rs"]
mod explore_flow;

const AGENT_E2E_AGENT_SOURCE: &str = "progressive-reviewer-e2e";

#[test]
fn source_loading_prefers_frozen_content_when_a_deleted_path_is_recreated() {
    let repository_files = repository_fixture(RepoType::Git);
    let deleted_content = b"fn deleted_from_worktree() {}\n";
    repository_files.write("deleted.rs", deleted_content);
    repository_files.new_change("add the file that the next change deletes");
    repository_files.remove("deleted.rs");
    let state_directory = tempfile::tempdir().unwrap();
    let repository = Repository::discover(repository_files.root())
        .unwrap()
        .with_state_root(state_directory.path());
    let snapshot = complete_repository_snapshot(&repository);
    let tracker = ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state_directory.path(), repository.root()).unwrap(),
    );
    let location = SourceLocation {
        path: repository.root().join("deleted.rs"),
        line: 0,
        byte_column: 0,
        end_line: 0,
        end_byte_column: 0,
    };
    let (commands, _command_receiver) = mpsc::channel();
    let (message_sender, messages) = application_message_channel();
    let store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
    let worker = Worker {
        repository: repository.clone(),
        tracker: Arc::new(tracker),
        explore: offline_explore_session(&repository, &store, &message_sender),
        store,
        snapshot: Some(snapshot.clone()),
        commands,
        exclusion: ExclusionPolicy::disabled(),
        auto_review: None,
        documents: mpsc::channel().0,
    };
    std::fs::write(&location.path, "fn recreated_after_snapshot() {}\n").unwrap();

    document_worker(&worker).load_source(
        &message_sender,
        snapshot.identity.snapshot_id().to_owned(),
        location.clone(),
        SourceLoadMode::External,
    );

    let event = messages.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        event.downcast_ref::<SourceContentLoaded>(),
        Some(SourceContentLoaded {
            snapshot_id,
            location: loaded_location,
            content,
            mode: SourceLoadMode::External,
        }) if snapshot_id == snapshot.identity.snapshot_id()
            && *loaded_location == location
            && content == deleted_content
    ));
    document_worker(&worker).load_source(
        &message_sender,
        snapshot.identity.snapshot_id().to_owned(),
        location.clone(),
        SourceLoadMode::ThreadPeek,
    );
    let event = messages.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(
        event.downcast_ref::<SourceContentLoaded>().unwrap().content,
        b"fn recreated_after_snapshot() {}\n"
    );
    std::fs::remove_file(&location.path).unwrap();
    document_worker(&worker).load_source(
        &message_sender,
        snapshot.identity.snapshot_id().to_owned(),
        location,
        SourceLoadMode::ThreadPeek,
    );
    let event = messages.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(event.downcast_ref::<SourceContentLoadFailed>().is_some());
}

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
        let deadline = Instant::now() + Duration::from_secs(5);
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
        let deadline = Instant::now() + Duration::from_secs(5);
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
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
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
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
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
    repository_files: Box<dyn ReviewRepositoryFixture>,
    state_directory: tempfile::TempDir,
    repository: Repository,
    herdr: IsolatedHerdrServer,
    commands: Sender<WorkerCommand>,
    messages: ApplicationMessageReceiver,
    worker_thread: JoinHandle<()>,
    review_unit: ReviewUnit,
    endpoint: review_mcp::Endpoint,
    comments: comments::Worker,
    _port: review_test_support::TestPort,
}

impl ReviewFlowFixture {
    fn start(repository_type: RepoType) -> Self {
        let repository_files = repository_fixture(repository_type);
        repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let state_directory = tempfile::tempdir().unwrap();
        let repository = Repository::discover(repository_files.root())
            .unwrap()
            .with_state_root(state_directory.path());
        let herdr = IsolatedHerdrServer::start_native(repository_files.root());
        let store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
        let tracker = ReviewTracker::new(repository.clone(), store.clone());
        let (commands, command_receiver) = mpsc::channel();
        let port = review_test_support::TestPort::new();
        let endpoint =
            review_mcp::Endpoint::for_repository(repository_files.root(), Some(port.number()))
                .unwrap();
        let comments = comments::Worker::start(
            store.clone(),
            herdr.client(),
            AgentTarget::new(herdr.workspace_id.clone(), Some(herdr.pane_id.clone())),
            Ok(endpoint),
            explore_route(commands.clone()),
            |_| {},
        );
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
        let (message_sender, messages) = application_message_channel();
        let mut worker = Worker {
            repository: repository.clone(),
            tracker: Arc::new(tracker),
            explore: live_explore_session(
                &repository,
                &store,
                &herdr,
                &comments,
                &commands,
                &message_sender,
            ),
            store,
            snapshot: None,
            commands: commands.clone(),
            exclusion: ExclusionPolicy::disabled(),
            auto_review: None,
            documents: mpsc::channel().0,
        };
        let worker_thread = thread::spawn(move || worker.run(&command_receiver, &message_sender));
        commands.send(WorkerCommand::Poll).unwrap();
        let review_unit = loop {
            let envelope = messages.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some(RepositoryMetadataChanged {
                review_checkpoint, ..
            }) = envelope.downcast_ref::<RepositoryMetadataChanged>()
            {
                break review_checkpoint.review_unit.clone();
            }
        };
        Self {
            repository_files,
            state_directory,
            repository,
            herdr,
            commands,
            messages,
            worker_thread,
            review_unit,
            endpoint,
            comments,
            _port: port,
        }
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn disk_content_changes_replace_the_visible_diff(repository_type: RepoType) {
    let repository_files = repository_fixture(repository_type);
    repository_files.write("src/lib.rs", b"pub fn before_refresh() {}\n");
    let state_directory = tempfile::tempdir().unwrap();
    let repository = Repository::discover(repository_files.root())
        .unwrap()
        .with_state_root(state_directory.path());
    let store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
    let tracker = ReviewTracker::new(repository.clone(), store);
    let (commands, _command_receiver) = mpsc::channel();
    let (message_sender, messages) = application_message_channel();
    let store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
    let mut worker = Worker {
        repository: repository.clone(),
        tracker: Arc::new(tracker),
        explore: offline_explore_session(&repository, &store, &message_sender),
        store,
        snapshot: None,
        commands,
        exclusion: ExclusionPolicy::disabled(),
        auto_review: None,
        documents: mpsc::channel().0,
    };
    let mut application = ReviewApplication::default();

    assert!(worker.poll(&message_sender));
    let initial_actions = publish_pending_worker_events(&mut application, &messages);
    load_requested_diffs(&worker, &message_sender, initial_actions);
    publish_pending_worker_events(&mut application, &messages);
    assert!(rendered_review_application(&application).contains("before_refresh"));

    repository_files.write("src/lib.rs", b"pub fn after_refresh() {}\n");
    assert!(worker.poll(&message_sender));
    let refresh_actions = publish_pending_worker_events(&mut application, &messages);
    assert!(matches!(
        refresh_actions.as_slice(),
        [Action::LoadDiff { .. }]
    ));
    load_requested_diffs(&worker, &message_sender, refresh_actions);
    publish_pending_worker_events(&mut application, &messages);

    let rendered = rendered_review_application(&application);
    assert!(rendered.contains("after_refresh"), "{rendered}");
    assert!(!rendered.contains("before_refresh"), "{rendered}");
}

/// A session for workers whose tests never prompt an agent.
pub(super) fn offline_explore_session(
    repository: &Repository,
    store: &ReviewStore,
    events: &ApplicationEventSender,
) -> ExploreSession {
    ExploreSession::new(explore_session::Collaborators {
        repository: repository.clone(),
        store: store.clone(),
        agents: Arc::new(HerdrClient::new(
            "/nonexistent/reviewer-test.sock".into(),
            "reviewer-test".into(),
            "/nonexistent".into(),
        )),
        target: AgentTarget::new(WorkspaceId("test".into()), None),
        prompts: comment_service::test_worker(store).prompt_sender(),
        exclusion: ExclusionPolicy::disabled(),
        events: events.clone(),
        inbox: explore_inbox(mpsc::channel().0),
    })
}

/// A session prompting the isolated Herdr agent through `comments`.
fn live_explore_session(
    repository: &Repository,
    store: &ReviewStore,
    herdr: &IsolatedHerdrServer,
    comments: &comments::Worker,
    commands: &Sender<WorkerCommand>,
    events: &ApplicationEventSender,
) -> ExploreSession {
    ExploreSession::new(explore_session::Collaborators {
        repository: repository.clone(),
        store: store.clone(),
        agents: Arc::new(herdr.client()),
        target: AgentTarget::new(herdr.workspace_id.clone(), Some(herdr.pane_id.clone())),
        prompts: comments.prompt_sender(),
        exclusion: ExclusionPolicy::disabled(),
        events: events.clone(),
        inbox: explore_inbox(commands.clone()),
    })
}

/// Deliver agent Explore calls to the worker, as the running reviewer does.
fn explore_route(
    commands: Sender<WorkerCommand>,
) -> impl Fn(review_mcp::Request) -> Result<(), String> + Send + Sync + 'static {
    move |request| {
        commands
            .send(WorkerCommand::Explore(explore_session::Input::Submission(
                Box::new(request),
            )))
            .map_err(|_| "The reviewer is closed".to_owned())
    }
}

fn publish_pending_worker_events(
    application: &mut ReviewApplication,
    messages: &ApplicationMessageReceiver,
) -> Vec<Action> {
    messages
        .try_iter()
        .flat_map(|event| application.publish_envelope(&event))
        .collect()
}

fn load_requested_diffs(worker: &Worker, messages: &ApplicationEventSender, actions: Vec<Action>) {
    for action in actions {
        if let Action::LoadDiff {
            review_checkpoint,
            path,
        } = action
        {
            document_worker(worker).load_diff(messages, review_checkpoint, path);
        }
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
fn dispatch_reports_that_quit_stops_the_runtime() {
    let repository = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let settings = ReviewStore::open(state.path(), repository.path()).unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (commands, _command_receiver) = mpsc::channel();

    let search = text_search::Worker::start(|_| {});
    let dispatcher = RuntimeActionDispatcher {
        source_watches: None,
        comments: &comment_service::test_worker(&settings),
        highlighting: &highlighting_worker(),
        search: &search,
        commands: &commands,
        documents: &mpsc::channel().0,
        settings: &settings,
        repository_root: repository.path(),
        lsp: &lsp,
    };

    assert!(dispatcher.dispatch(Action::Quit).unwrap());
}

#[test]
fn dispatch_all_executes_earlier_actions_before_quit() {
    let repository = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let settings = ReviewStore::open(state.path(), repository.path()).unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (commands, _command_receiver) = mpsc::channel();

    let search = text_search::Worker::start(|_| {});
    let dispatcher = RuntimeActionDispatcher {
        source_watches: None,
        comments: &comment_service::test_worker(&settings),
        highlighting: &highlighting_worker(),
        search: &search,
        commands: &commands,
        documents: &mpsc::channel().0,
        settings: &settings,
        repository_root: repository.path(),
        lsp: &lsp,
    };

    assert!(
        dispatcher
            .dispatch_all(vec![Action::SaveFilePaneWidth(42), Action::Quit], |_, _| {
                unreachable!("no editor action")
            })
            .unwrap()
    );
    assert_eq!(settings.file_pane_width().unwrap(), Some(42));
}

#[test]
fn event_loop_routes_external_events_from_the_central_channel() {
    let repository = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let settings = ReviewStore::open(state.path(), repository.path()).unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut app = ReviewApplication::default();
    let (commands, command_receiver) = mpsc::channel();
    let (event_sender, events) = unbounded();
    let focused_pane = PaneId("focused-agent".to_owned());
    event_sender
        .send(EventEnvelope::new(HerdrEvent::PaneFocused(
            focused_pane.clone(),
        )))
        .unwrap();
    event_sender
        .send(EventEnvelope::new(RepositoryRefreshDue))
        .unwrap();
    event_sender
        .send(EventEnvelope::new(ApplicationTick(Instant::now())))
        .unwrap();
    event_sender
        .send(EventEnvelope::new(StopRequested))
        .unwrap();

    RuntimeEventLoop {
        target: AgentTarget::new(herdr_client::protocol::WorkspaceId("test".into()), None),
        source_watches: None,
        terminal_events: None,
        last_frame: Instant::now(),
        comments: &comment_service::test_worker(&settings),
        highlighting: &highlighting_worker(),
        timings: &timing::Recorder::default(),
        search: &text_search::Worker::start(|_| {}),
        terminal: &mut terminal,
        app: &mut app,
        commands: &commands,
        documents: &mpsc::channel().0,
        events: &mut events::Inbox::new(events, crossbeam_channel::never()),
        lsp: &lsp,
        repository_root: repository.path(),
        settings: &settings,
    }
    .run()
    .unwrap();

    let commands = command_receiver.try_iter().collect::<Vec<_>>();
    assert!(matches!(commands.as_slice(), [WorkerCommand::Poll]));
}

#[test]
fn runtime_event_producers_join_active_lsp_adapter() {
    let repository = tempfile::tempdir().unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (event_sender, events) = unbounded();
    let stop_requested = Arc::new(AtomicBool::new(false));
    let mut producers = RuntimeEventProducers::new(Arc::clone(&stop_requested));
    producers.push(Runtime::start_lsp_events(
        &lsp,
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
        .recv_timeout(Duration::from_secs(1))
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
            let event = events.recv_timeout(Duration::from_secs(1)).unwrap();
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

fn document_worker(worker: &Worker) -> document::DocumentWorker {
    document::DocumentWorker {
        repository: worker.repository.clone(),
        tracker: Arc::clone(&worker.tracker),
        snapshot: worker.snapshot.clone(),
    }
}

#[test]
fn document_requests_complete_while_repository_work_is_pending() {
    let files = repository_fixture(RepoType::Git);
    files.write("changed.rs", b"fn original() {}\n");
    files.new_change("original");
    files.write("changed.rs", b"fn updated() {}\n");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(files.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let checkpoint = ReviewCheckpoint::new(
        snapshot.identity.review_unit().clone(),
        snapshot.identity.snapshot_id(),
    );
    let settings = ReviewStore::open(state.path(), repository.root()).unwrap();
    let tracker = Arc::new(ReviewTracker::new(
        repository.clone(),
        ReviewStore::open(state.path(), repository.root()).unwrap(),
    ));
    let mut worker = document::DocumentWorker {
        repository: repository.clone(),
        tracker,
        snapshot: None,
    };
    let (documents, document_receiver) = mpsc::channel();
    let (sender, messages) = application_message_channel();
    let document_thread = thread::spawn(move || worker.run(&document_receiver, &sender));
    documents
        .send(document::Command::Snapshot(snapshot))
        .unwrap();
    // Leave repository work pending throughout the file requests.
    let (commands, command_receiver) = mpsc::channel();
    commands.send(WorkerCommand::Poll).unwrap();
    let lsp = review_lsp::Worker::start(repository.root().to_owned());
    let search = text_search::Worker::start(|_| {});
    let dispatcher = RuntimeActionDispatcher {
        source_watches: None,
        comments: &comment_service::test_worker(&settings),
        highlighting: &highlighting_worker(),
        search: &search,
        commands: &commands,
        documents: &documents,
        settings: &settings,
        repository_root: repository.root(),
        lsp: &lsp,
    };
    for action in [
        Action::LoadDiff {
            review_checkpoint: checkpoint.clone(),
            path: "changed.rs".to_owned(),
        },
        Action::LoadDiffs {
            review_checkpoint: checkpoint.clone(),
            paths: vec!["changed.rs".to_owned()],
        },
    ] {
        dispatcher.dispatch(action).unwrap();
        let event = messages.recv_timeout(Duration::from_secs(5)).unwrap();
        let loaded = event.downcast_ref::<DiffContentLoaded>().unwrap();
        assert_eq!(loaded.review_checkpoint, checkpoint);
        assert_eq!(
            loaded.new_content.as_deref(),
            Some(b"fn updated() {}\n".as_slice())
        );
    }
    dispatcher
        .dispatch(Action::LoadSource {
            snapshot_id: checkpoint.checkpoint.clone(),
            location: SourceLocation {
                path: PathBuf::from("changed.rs"),
                line: 0,
                byte_column: 0,
                end_line: 0,
                end_byte_column: 0,
            },
            mode: SourceLoadMode::External,
        })
        .unwrap();
    let event = messages.recv_timeout(Duration::from_secs(5)).unwrap();
    let loaded = event.downcast_ref::<SourceContentLoaded>().unwrap();
    assert_eq!(loaded.content, b"fn updated() {}\n");
    assert_eq!(loaded.location.path, repository.root().join("changed.rs"));
    assert!(matches!(
        command_receiver.try_recv(),
        Ok(WorkerCommand::Poll)
    ));
    assert!(command_receiver.try_recv().is_err());
    documents.send(document::Command::Quit).unwrap();
    document_thread.join().unwrap();
}

fn highlighting_worker() -> highlighting::Worker {
    let theme = Theme::default();
    highlighting::Worker::start(
        syntax_highlighting::SyntaxHighlighter::new(theme.syntax, theme.palette.text),
        |_| {},
    )
}
