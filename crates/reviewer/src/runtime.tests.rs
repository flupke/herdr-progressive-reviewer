use super::*;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use herdr_client::protocol::HerdrReader;
use ratatui::backend::TestBackend;
use review_repository::diff::DiffRow;
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};

#[path = "runtime/mcp.tests.rs"]
mod mcp;

#[path = "runtime/agent_input.tests.rs"]
mod agent_input;
#[path = "runtime/explore.tests.rs"]
mod explore_flow;

const GUIDE_E2E_AGENT_SOURCE: &str = "progressive-reviewer-e2e";
// Test against the installed binary's rules, without background network updates.
const GUIDE_E2E_CONFIG: &str = "onboarding = false\n\
    [update]\nversion_check = false\nmanifest_check = false\n";

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
    let worker = Worker {
        repository: repository.clone(),
        tracker: Arc::new(tracker),
        guide_store: ReviewStore::open(state_directory.path(), repository.root()).unwrap(),
        client: HerdrClient::new(
            state_directory.path().join("unused.sock"),
            "progressive-reviewer-test".to_owned(),
            state_directory.path().to_owned(),
        ),
        target: AgentTarget::new(WorkspaceId("test-workspace".to_owned()), None),
        snapshot: Some(snapshot.clone()),
        commands,
        guide: guide::GuideRequestCoordinator::default(),
        explore: explore::ExploreRuntime::default(),
        prompts: comment_service::test_worker(
            &ReviewStore::open(state_directory.path(), repository.root()).unwrap(),
        )
        .prompt_sender(),
        documents: mpsc::channel().0,
    };
    std::fs::write(&location.path, "fn recreated_after_snapshot() {}\n").unwrap();
    let (message_sender, messages) = application_message_channel();

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
    directory: tempfile::TempDir,
    binary: PathBuf,
    socket_path: PathBuf,
    state_directory: PathBuf,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    agent_binary: PathBuf,
    agent: String,
    child: Child,
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
        let directory = tempfile::tempdir().unwrap();
        let config_directory = directory.path().join("config");
        let runtime_directory = directory.path().join("runtime");
        let state_directory = directory.path().join("state");
        let config_path = config_directory.join("herdr/config.toml");
        let socket_path = directory.path().join("herdr.sock");
        let prompt_path = directory.path().join("prompt.txt");
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::create_dir_all(&runtime_directory).unwrap();
        fs::create_dir_all(&state_directory).unwrap();
        fs::write(&config_path, GUIDE_E2E_CONFIG).unwrap();

        let binary = std::env::var_os("HERDR_BIN_PATH")
            .map_or_else(|| PathBuf::from("herdr"), PathBuf::from);
        let server_output = File::create(directory.path().join("server.log")).unwrap();
        let child = Command::new(&binary)
            .arg("server")
            .current_dir(repository_root)
            .env("HERDR_SOCKET_PATH", &socket_path)
            .env("HERDR_CONFIG_PATH", &config_path)
            .env("XDG_CONFIG_HOME", &config_directory)
            .env("XDG_RUNTIME_DIR", &runtime_directory)
            .env("XDG_STATE_HOME", &state_directory)
            .env("SHELL", "/bin/sh")
            .env_remove("HERDR_CLIENT_SOCKET_PATH")
            .env_remove("HERDR_ENV")
            .stdin(Stdio::null())
            .stdout(server_output.try_clone().unwrap())
            .stderr(server_output)
            .spawn()
            .unwrap_or_else(|error| panic!("could not start {}: {error}", binary.display()));
        let mut child = child;
        Self::wait_until_ready(&directory, &socket_path, &mut child);

        let prompt_environment = format!("REVIEW_GUIDE_E2E_PROMPT_PATH={}", prompt_path.display());
        let binary_environment = format!("REVIEW_GUIDE_E2E_HERDR_BIN={}", binary.display());
        let agent_environment = format!("REVIEW_GUIDE_E2E_AGENT={agent}");
        let session_environment = format!(
            "REVIEW_GUIDE_E2E_AGENT_SESSION={}",
            session.unwrap_or_default()
        );
        let report_environment = format!(
            "REVIEW_GUIDE_E2E_REPORT_LIFECYCLE={}",
            match lifecycle {
                AgentLifecycle::Reported => "1",
                AgentLifecycle::Native => "0",
            }
        );
        let workspace = Self::run_cli_json_with(
            &binary,
            &socket_path,
            &[
                "workspace",
                "create",
                "--cwd",
                &repository_root.to_string_lossy(),
                "--label",
                "review-guide-e2e",
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
            ],
        );
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
        let agent_binary = directory.path().join(agent);
        fs::copy(current_test_binary, &agent_binary).unwrap();
        let server = Self {
            directory,
            binary,
            socket_path,
            state_directory,
            workspace_id,
            pane_id,
            agent_binary,
            agent: agent.into(),
            child,
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
            "runtime::tests::guide_e2e_agent_process",
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
        HerdrClient::new(
            self.socket_path.clone(),
            "herdr.progressive-reviewer".to_owned(),
            self.state_directory.clone(),
        )
    }

    fn wait_until_ready(directory: &tempfile::TempDir, socket_path: &Path, child: &mut Child) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if socket_path.exists() && std::os::unix::net::UnixStream::connect(socket_path).is_ok()
            {
                return;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(25));
        }
        let log = fs::read_to_string(directory.path().join("server.log")).unwrap_or_default();
        panic!("isolated Herdr server did not become ready:\n{log}");
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

    fn run_cli_json_with(
        binary: &Path,
        socket_path: &Path,
        arguments: &[&str],
    ) -> serde_json::Value {
        let output = Self::run_cli_with(binary, socket_path, arguments);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn run_cli(&self, arguments: &[&str]) -> std::process::Output {
        Self::run_cli_with(&self.binary, &self.socket_path, arguments)
    }

    fn run_cli_with(binary: &Path, socket_path: &Path, arguments: &[&str]) -> std::process::Output {
        let output = Command::new(binary)
            .args(arguments)
            .env("HERDR_SOCKET_PATH", socket_path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "herdr {} failed:\n{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn report_agent(&self, state: &str) {
        self.run_cli(&[
            "pane",
            "report-agent",
            &self.pane_id.0,
            "--source",
            GUIDE_E2E_AGENT_SOURCE,
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
            GUIDE_E2E_AGENT_SOURCE,
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

impl Drop for IsolatedHerdrServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn guide_e2e_agent_process() {
    let Some(prompt_path) = std::env::var_os("REVIEW_GUIDE_E2E_PROMPT_PATH") else {
        return;
    };
    // Match a native TUI: the terminal must not submit pasted newlines, echo
    // input, or truncate long lines through its canonical input buffer.
    crossterm::terminal::enable_raw_mode().unwrap();
    crossterm::execute!(io::stdout(), crossterm::event::EnableBracketedPaste).unwrap();
    let binary = std::env::var_os("REVIEW_GUIDE_E2E_HERDR_BIN").unwrap();
    let pane_id = std::env::var("HERDR_PANE_ID").unwrap();
    let agent = std::env::var("REVIEW_GUIDE_E2E_AGENT").unwrap();
    if std::env::var("REVIEW_GUIDE_E2E_REPORT_LIFECYCLE").as_deref() != Ok("0") {
        let status = Command::new(&binary)
            .args([
                "pane",
                "report-agent",
                &pane_id,
                "--source",
                GUIDE_E2E_AGENT_SOURCE,
                "--agent",
                &agent,
                "--state",
                "idle",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    if let Ok(session) = std::env::var("REVIEW_GUIDE_E2E_AGENT_SESSION")
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

fn prompt_field(prompt: &str, label: &str) -> String {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .and_then(|value| value.strip_prefix('`'))
        .and_then(|value| value.strip_suffix('`'))
        .unwrap()
        .to_owned()
}

fn write_guide_response(prompt: &str, text: &str) {
    let temporary_path = PathBuf::from(prompt_field(prompt, "- Temporary response: "));
    let response_path = PathBuf::from(prompt_field(prompt, "- Final response: "));
    std::fs::write(
        &temporary_path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "items": [{
                "target": {
                    "kind": "lines",
                    "path": "reviewed.rs",
                    "old": null,
                    "new": {
                        "first_line": 1,
                        "last_line": 1,
                    },
                },
                "text": text,
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::rename(temporary_path, response_path).unwrap();
}

fn respond_to_delivered_guide(
    prompt_path: PathBuf,
    prompt_offset: u64,
    response_text: String,
    delivered: mpsc::SyncSender<()>,
    write_response: Receiver<()>,
) -> JoinHandle<u64> {
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let prompt_offset = usize::try_from(prompt_offset).unwrap();
        while Instant::now() < deadline {
            if let Ok(all_prompts) = fs::read_to_string(&prompt_path)
                && let Some(prompt) = all_prompts.get(prompt_offset..)
                && prompt.contains("- Final response: `")
            {
                delivered.send(()).unwrap();
                write_response.recv().unwrap();
                write_guide_response(prompt, &response_text);
                return fs::metadata(prompt_path).unwrap().len();
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("Herdr did not deliver the review-guide prompt to the agent");
    })
}

fn receive_guide(
    messages: &ApplicationMessageReceiver,
    expected_review_unit: &ReviewUnit,
    expected_checkpoint: &str,
    expected_text: &str,
) {
    loop {
        let envelope = messages.recv_timeout(Duration::from_secs(5)).unwrap();
        if let Some(event) = envelope.downcast_ref::<ReviewGuideChanged>() {
            assert!(
                event
                    .review_checkpoint
                    .matches(expected_review_unit, expected_checkpoint)
            );
            assert_eq!(event.items.len(), 1);
            assert_eq!(event.items[0].text, expected_text);
            return;
        }
        if let Some(event) = envelope.downcast_ref::<ui_events::ReviewGuideStatusChanged>()
            && !event.generating
            && event.message.is_some()
        {
            panic!("guide request failed: {:?}", event.message)
        }
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

fn forward_until_agent_detection(events: &Receiver<HerdrEvent>, expected_released: bool) {
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| {
                panic!("waiting for agent detection with released={expected_released}: {error}")
            });
        let is_expected = matches!(
            event,
            HerdrEvent::AgentDetected { released, .. } if released == expected_released
        );
        if is_expected {
            return;
        }
    }
}

fn churn_agent_lifecycle(herdr: &IsolatedHerdrServer, events: &Receiver<HerdrEvent>) {
    herdr.stop_agent();
    forward_until_agent_detection(events, true);
    herdr.start_agent();
    herdr.wait_for_agent(None);
    forward_until_agent_detection(events, false);
}

struct GuideFlowFixture {
    repository_files: Box<dyn ReviewRepositoryFixture>,
    state_directory: tempfile::TempDir,
    repository: Repository,
    herdr: IsolatedHerdrServer,
    commands: Sender<WorkerCommand>,
    messages: ApplicationMessageReceiver,
    worker_thread: JoinHandle<()>,
    events: Receiver<HerdrEvent>,
    review_unit: ReviewUnit,
    checkpoint: String,
    prompt_length: u64,
    endpoint: review_mcp::Endpoint,
    comments: comments::Worker,
    _port: review_test_support::TestPort,
}

impl GuideFlowFixture {
    fn start(repository_type: RepoType) -> Self {
        let repository_files = repository_fixture(repository_type);
        repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let state_directory = tempfile::tempdir().unwrap();
        let repository = Repository::discover(repository_files.root())
            .unwrap()
            .with_state_root(state_directory.path());
        let herdr = IsolatedHerdrServer::start_native(repository_files.root());
        let store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
        let guide_store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
        let tracker = ReviewTracker::new(repository.clone(), store);
        let (commands, command_receiver) = mpsc::channel();
        let port = review_test_support::TestPort::new();
        let endpoint =
            review_mcp::Endpoint::for_repository(repository_files.root(), Some(port.number()))
                .unwrap();
        let prompt_commands = commands.clone();
        let comments = comments::Worker::start(
            guide_store.clone(),
            herdr.client(),
            AgentTarget::new(herdr.workspace_id.clone(), Some(herdr.pane_id.clone())),
            Ok(endpoint),
            move |event| {
                if let comments::Event::Explore(request) = event {
                    let _ = prompt_commands.send(WorkerCommand::ExploreMcp(Box::new(request)));
                }
            },
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
        let mut worker = Worker {
            repository: repository.clone(),
            tracker: Arc::new(tracker),
            guide_store,
            client: herdr.client(),
            target: AgentTarget::new(herdr.workspace_id.clone(), Some(herdr.pane_id.clone())),
            snapshot: None,
            commands: commands.clone(),
            guide: guide::GuideRequestCoordinator::default(),
            explore: explore::ExploreRuntime::default(),
            prompts: comments.prompt_sender(),
            documents: mpsc::channel().0,
        };
        let (message_sender, messages) = application_message_channel();
        let worker_thread = thread::spawn(move || worker.run(&command_receiver, &message_sender));

        commands.send(WorkerCommand::Poll).unwrap();
        let (review_unit, checkpoint) = loop {
            let envelope = messages.recv_timeout(Duration::from_secs(5)).unwrap();
            if let Some(RepositoryMetadataChanged {
                review_checkpoint, ..
            }) = envelope.downcast_ref::<RepositoryMetadataChanged>()
            {
                break (
                    review_checkpoint.review_unit.clone(),
                    review_checkpoint.checkpoint.clone(),
                );
            }
        };
        let events = AgentEventSubscription::start(&herdr).events;

        Self {
            endpoint,
            comments,
            _port: port,
            repository_files,
            state_directory,
            repository,
            herdr,
            commands,
            messages,
            worker_thread,
            events,
            review_unit,
            checkpoint,
            prompt_length: 0,
        }
    }

    fn request_first_guide_after_agent_churn(&mut self) {
        self.request_guide("First explanation", |fixture| {
            churn_agent_lifecycle(&fixture.herdr, &fixture.events);
        });
    }

    fn request_guide(&mut self, response_text: &str, after_prompt_delivery: impl FnOnce(&Self)) {
        self.herdr.report_agent("idle");
        let (prompt_delivered_sender, prompt_delivered_receiver) = mpsc::sync_channel(0);
        let (write_response_sender, write_response_receiver) = mpsc::channel();
        let response_writer = respond_to_delivered_guide(
            self.herdr.directory.path().join("prompt.txt"),
            self.prompt_length,
            response_text.to_owned(),
            prompt_delivered_sender,
            write_response_receiver,
        );
        self.commands
            .send(WorkerCommand::GenerateReviewGuide(GuideScope::All))
            .unwrap();
        prompt_delivered_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        after_prompt_delivery(self);
        write_response_sender.send(()).unwrap();
        receive_guide(
            &self.messages,
            &self.review_unit,
            &self.checkpoint,
            response_text,
        );
        self.prompt_length = response_writer.join().unwrap();
    }

    fn confirm_refresh_does_not_generate_guide(&self) {
        self.commands.send(WorkerCommand::Poll).unwrap();
        receive_guide(
            &self.messages,
            &self.review_unit,
            &self.checkpoint,
            "First explanation",
        );
        assert_eq!(
            fs::metadata(self.herdr.directory.path().join("prompt.txt"))
                .unwrap()
                .len(),
            self.prompt_length,
            "repository refresh must not request another guide"
        );
    }

    fn replace_guide(&mut self) {
        self.request_guide("Replacement explanation", |_| {});
    }

    fn assert_replacement_is_stored(self) {
        let Self {
            repository_files,
            state_directory,
            repository,
            herdr,
            commands,
            messages: _,
            worker_thread,
            events,
            review_unit,
            checkpoint,
            prompt_length: _,
            comments,
            _port,
            endpoint: _,
        } = self;
        drop(events);
        commands.send(WorkerCommand::Quit).unwrap();
        worker_thread.join().unwrap();
        let stored = ReviewStore::open(state_directory.path(), repository.root())
            .unwrap()
            .load_guide(&review_unit)
            .unwrap()
            .unwrap();
        assert_eq!(stored.review_checkpoint.checkpoint, checkpoint);
        assert_eq!(stored.items.len(), 1);
        assert_eq!(stored.items[0].text, "Replacement explanation");
        drop((comments, herdr, repository_files));
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn guide_flow_survives_agent_churn_and_replaces_results(repository_type: RepoType) {
    let mut fixture = GuideFlowFixture::start(repository_type);
    fixture.request_first_guide_after_agent_churn();
    fixture.confirm_refresh_does_not_generate_guide();
    fixture.replace_guide();
    fixture.assert_replacement_is_stored();
}

#[test]
fn replacing_a_pending_guide_ignores_its_response_when_preparation_fails() {
    let mut fixture = GuideFlowFixture::start(RepoType::Git);
    fixture
        .commands
        .send(WorkerCommand::GenerateReviewGuide(GuideScope::All))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let first_prompt = loop {
        let prompt = fs::read_to_string(fixture.herdr.directory.path().join("prompt.txt"))
            .unwrap_or_default();
        if prompt.contains("- Final response: `") {
            break prompt;
        }
        assert!(Instant::now() < deadline, "guide prompt was not delivered");
        thread::sleep(Duration::from_millis(25));
    };
    fixture.prompt_length = u64::try_from(first_prompt.len()).unwrap();
    fixture
        .commands
        .send(WorkerCommand::GenerateReviewGuide(GuideScope::File {
            path: "missing.rs".into(),
        }))
        .unwrap();
    loop {
        let event = fixture
            .messages
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        if let Some(status) = event.downcast_ref::<ui_events::ReviewGuideStatusChanged>()
            && !status.generating
        {
            assert!(
                status
                    .message
                    .as_ref()
                    .unwrap()
                    .contains("no visible unreviewed file")
            );
            break;
        }
    }
    // The failed replacement still supersedes the old response watcher.
    write_guide_response(&first_prompt, "Late answer to a cancelled request");
    thread::sleep(Duration::from_millis(350));
    assert_eq!(
        fs::read_to_string(fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
        first_prompt
    );
    assert!(!fixture.messages.try_iter().any(|event| {
        event
            .downcast_ref::<ui_events::ReviewGuideStatusChanged>()
            .is_some()
    }));
    fixture.replace_guide();
    fixture.assert_replacement_is_stored();
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
    let mut worker = Worker {
        repository: repository.clone(),
        tracker: Arc::new(tracker),
        guide_store: ReviewStore::open(state_directory.path(), repository.root()).unwrap(),
        client: HerdrClient::new(
            state_directory.path().join("unused.sock"),
            "progressive-reviewer-test".to_owned(),
            state_directory.path().to_owned(),
        ),
        target: AgentTarget::new(WorkspaceId("test-workspace".to_owned()), None),
        snapshot: None,
        commands,
        guide: guide::GuideRequestCoordinator::default(),
        explore: explore::ExploreRuntime::default(),
        prompts: comment_service::test_worker(
            &ReviewStore::open(state_directory.path(), repository.root()).unwrap(),
        )
        .prompt_sender(),
        documents: mpsc::channel().0,
    };
    let (message_sender, messages) = application_message_channel();
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

fn publish_pending_worker_events(
    application: &mut ReviewApplication,
    messages: &ApplicationMessageReceiver,
) -> Vec<Action> {
    messages
        .try_iter()
        .flat_map(|event| application.publish_envelope(&event))
        .collect()
}

fn load_requested_diffs(
    worker: &Worker,
    messages: &ApplicationMessageSender,
    actions: Vec<Action>,
) {
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
        normalize_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)),
        Some(Key::Char('V'))
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE)),
        Some(Key::Char('l'))
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
        Some(Key::Char('c'))
    );
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
fn control_location_keys_use_location_history_actions() {
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
        Some(Key::HalfPageDown)
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
        Some(Key::HalfPageUp)
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
        Some(Key::PreviousLocation)
    );
    assert_eq!(
        normalize_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL)),
        Some(Key::NextLocation)
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
            .dispatch_all(vec![Action::SaveFilePaneWidth(42), Action::Quit])
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
            Event::Key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)),
            Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE)),
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
