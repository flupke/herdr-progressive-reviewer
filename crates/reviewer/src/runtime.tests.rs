use super::*;
use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use ratatui::layout::Rect;
use ratatui::{TerminalOptions, Viewport, backend::TestBackend};
use review_repository::diff::DiffRow;
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};

const GUIDE_E2E_AGENT_SOURCE: &str = "progressive-reviewer-e2e";
const GUIDE_E2E_AGENT_SESSION_SOURCE: &str = "herdr:codex";

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
        tracker,
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
    };
    std::fs::write(&location.path, "fn recreated_after_snapshot() {}\n").unwrap();
    let (message_sender, messages) = application_message_channel();

    worker.load_source(
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
}

struct IsolatedHerdrServer {
    directory: tempfile::TempDir,
    binary: PathBuf,
    socket_path: PathBuf,
    state_directory: PathBuf,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    agent_binary: PathBuf,
    child: Child,
}

impl IsolatedHerdrServer {
    fn start(repository_root: &std::path::Path) -> Self {
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
        fs::write(&config_path, "onboarding = false\n").unwrap();

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
        let agent_binary = directory.path().join("codex");
        fs::copy(current_test_binary, &agent_binary).unwrap();
        let server = Self {
            directory,
            binary,
            socket_path,
            state_directory,
            workspace_id,
            pane_id,
            agent_binary,
            child,
        };
        server.start_agent();
        server.wait_for_agent();
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

    fn wait_for_agent(&self) {
        let client = self.client();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if client
                .get_agent(&self.pane_id)
                .is_ok_and(|agent| agent.is_some_and(|agent| agent.agent_session.is_some()))
            {
                return;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("the deterministic agent did not register with Herdr");
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
            "codex",
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
            "codex",
        ]);
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
    let binary = std::env::var_os("REVIEW_GUIDE_E2E_HERDR_BIN").unwrap();
    let pane_id = std::env::var("HERDR_PANE_ID").unwrap();
    for arguments in [
        vec![
            "pane",
            "report-agent",
            &pane_id,
            "--source",
            GUIDE_E2E_AGENT_SOURCE,
            "--agent",
            "codex",
            "--state",
            "idle",
        ],
        vec![
            "pane",
            "report-agent-session",
            &pane_id,
            "--source",
            GUIDE_E2E_AGENT_SESSION_SOURCE,
            "--agent",
            "codex",
            "--agent-session-id",
            "session",
        ],
    ] {
        let status = Command::new(&binary).args(arguments).status().unwrap();
        assert!(status.success());
    }
    let mut prompt_file = File::create(prompt_path).unwrap();
    io::copy(&mut io::stdin().lock(), &mut prompt_file).unwrap();
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

fn confirm_multiple_event_subscribers(herdr: &IsolatedHerdrServer) {
    let (first_sender, first_events) = mpsc::channel();
    let (second_sender, second_events) = mpsc::channel();
    let first_client = herdr.client();
    let second_client = herdr.client();
    let first_pane_id = herdr.pane_id.clone();
    let second_pane_id = herdr.pane_id.clone();
    let first_thread = thread::spawn(move || {
        first_client.forward_events(&first_sender, std::slice::from_ref(&first_pane_id))
    });
    let second_thread = thread::spawn(move || {
        second_client.forward_events(&second_sender, std::slice::from_ref(&second_pane_id))
    });

    receive_current_agent(&first_events);
    receive_current_agent(&second_events);

    herdr.release_agent();
    receive_agent_release(&first_events);
    receive_agent_release(&second_events);
    drop(first_events);
    drop(second_events);
    herdr.report_agent("idle");
    assert_eq!(
        first_thread.join().unwrap().unwrap(),
        EventStreamEnd::ReceiverDisconnected
    );
    assert_eq!(
        second_thread.join().unwrap().unwrap(),
        EventStreamEnd::ReceiverDisconnected
    );
}

fn receive_current_agent(events: &Receiver<HerdrEvent>) {
    loop {
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        if matches!(
            event,
            HerdrEvent::AgentDetected {
                released: false,
                ..
            }
        ) {
            return;
        }
    }
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
    let client = herdr.client();
    let pane_id = herdr.pane_id.clone();
    let continue_streaming = Arc::new(AtomicBool::new(true));
    let thread_continue_streaming = Arc::clone(&continue_streaming);
    let (event_sender, events) = mpsc::channel();
    let thread = thread::spawn(move || {
        client.forward_events_while(
            std::slice::from_ref(&pane_id),
            || thread_continue_streaming.load(Ordering::Relaxed),
            |event| event_sender.send(event).is_ok(),
        )
    });
    receive_current_agent(&events);

    continue_streaming.store(false, Ordering::Relaxed);

    assert_eq!(
        thread.join().unwrap().unwrap(),
        EventStreamEnd::ReceiverDisconnected
    );
}

fn subscribe_to_agent_events(herdr: &IsolatedHerdrServer) -> Receiver<HerdrEvent> {
    let event_client = herdr.client();
    let event_pane_id = herdr.pane_id.clone();
    let (event_sender, event_receiver) = mpsc::channel();
    thread::spawn(move || {
        loop {
            let Ok(end) =
                event_client.forward_events(&event_sender, std::slice::from_ref(&event_pane_id))
            else {
                return;
            };
            if end == EventStreamEnd::ReceiverDisconnected {
                return;
            }
        }
    });
    event_receiver
}

fn forward_until_agent_detection(events: &Receiver<HerdrEvent>, expected_released: bool) {
    loop {
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        let is_expected = matches!(
            event,
            HerdrEvent::AgentDetected { released, .. } if released == expected_released
        );
        if is_expected {
            return;
        }
    }
}

fn confirm_agent_event_subscription(events: &Receiver<HerdrEvent>) {
    // Herdr replays the current detected agent after it accepts the subscription.
    forward_until_agent_detection(events, false);
}

fn churn_agent_lifecycle(herdr: &IsolatedHerdrServer, events: &Receiver<HerdrEvent>) {
    herdr.release_agent();
    herdr.report_agent("idle");
    forward_until_agent_detection(events, true);
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
}

impl GuideFlowFixture {
    fn start(repository_type: RepoType) -> Self {
        let repository_files = repository_fixture(repository_type);
        repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let state_directory = tempfile::tempdir().unwrap();
        let repository = Repository::discover(repository_files.root())
            .unwrap()
            .with_state_root(state_directory.path());
        let herdr = IsolatedHerdrServer::start(repository_files.root());
        let store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
        let guide_store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
        let tracker = ReviewTracker::new(repository.clone(), store);
        let (commands, command_receiver) = mpsc::channel();
        let mut worker = Worker {
            repository: repository.clone(),
            tracker,
            guide_store,
            client: herdr.client(),
            target: AgentTarget::new(herdr.workspace_id.clone(), Some(herdr.pane_id.clone())),
            snapshot: None,
            commands: commands.clone(),
            guide: guide::GuideRequestCoordinator::default(),
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
        let events = subscribe_to_agent_events(&herdr);
        confirm_agent_event_subscription(&events);

        Self {
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
        drop((herdr, repository_files));
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
        tracker,
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
            worker.load_diff(messages, review_checkpoint, path);
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
fn finds_a_nested_rust_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let crates = directory.path().join("crates");
    std::fs::create_dir(&crates).unwrap();
    std::fs::write(crates.join("Cargo.toml"), "[workspace]\n").unwrap();

    assert_eq!(rust_project_root(directory.path()), Some(crates));
}

#[test]
fn only_existing_rust_documents_are_opened_for_lsp() {
    let repository = tempfile::tempdir().unwrap();
    let source_directory = repository.path().join("src");
    std::fs::create_dir(&source_directory).unwrap();
    std::fs::write(source_directory.join("lib.rs"), "fn present() {}\n").unwrap();

    assert_eq!(
        rust_document_path(repository.path(), "src/lib.rs"),
        Some(source_directory.join("lib.rs"))
    );
    assert_eq!(
        rust_document_path(repository.path(), "src/deleted.rs"),
        None
    );
    assert_eq!(rust_document_path(repository.path(), "README.md"), None);
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
    for (kind, modifiers) in [
        (MouseEventKind::Down(MouseButton::Left), KeyModifiers::SHIFT),
        (
            MouseEventKind::Down(MouseButton::Middle),
            KeyModifiers::NONE,
        ),
    ] {
        assert_eq!(
            normalize_mouse(MouseEvent {
                kind,
                column: 4,
                row: 5,
                modifiers,
            }),
            Some(UserInput::MouseClick {
                column: 4,
                row: 5,
                insert_path: true,
            })
        );
    }
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

    let dispatcher = RuntimeActionDispatcher {
        commands: &commands,
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

    let dispatcher = RuntimeActionDispatcher {
        commands: &commands,
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
fn worker_command_preserves_output_actions() {
    let repository = tempfile::tempdir().unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (commands, _command_receiver) = mpsc::channel();

    let settings_directory = tempfile::tempdir().unwrap();
    let settings = ReviewStore::open(settings_directory.path(), repository.path()).unwrap();
    let dispatcher = RuntimeActionDispatcher {
        commands: &commands,
        settings: &settings,
        repository_root: repository.path(),
        lsp: &lsp,
    };

    let command = dispatcher
        .worker_command(Action::Output {
            text: "selected code".to_owned(),
        })
        .unwrap()
        .unwrap();

    assert!(matches!(
        command,
        WorkerCommand::Output {
            text,
        } if text == "selected code"
    ));
}

#[test]
fn event_loop_routes_external_events_from_the_central_channel() {
    let repository = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let settings = ReviewStore::open(state.path(), repository.path()).unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let mut terminal = TerminalGuard {
        terminal: Terminal::with_options(
            CrosstermBackend::new(stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 80, 20)),
            },
        )
        .unwrap(),
    };
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
        .send(EventEnvelope::new(RepositoryPollDue))
        .unwrap();
    event_sender
        .send(EventEnvelope::new(ApplicationTick(Instant::now())))
        .unwrap();
    event_sender
        .send(EventEnvelope::new(StopRequested))
        .unwrap();

    RuntimeEventLoop {
        terminal: &mut terminal,
        app: &mut app,
        commands: &commands,
        events,
        lsp: &lsp,
        lsp_root: repository.path(),
        repository_root: repository.path(),
        settings: &settings,
    }
    .run()
    .unwrap();

    let commands = command_receiver.try_iter().collect::<Vec<_>>();
    assert!(matches!(
        commands.as_slice(),
        [WorkerCommand::Focus(pane_id), WorkerCommand::Poll] if pane_id == &focused_pane
    ));
    std::mem::forget(terminal);
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
