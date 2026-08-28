use super::*;
use std::fs::{self, File};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use ratatui::layout::Rect;
use ratatui::{TerminalOptions, Viewport};
use review_repository::repository::RepoType;
use review_test_support::{ReviewRepositoryFixture, repository_fixture};

const GUIDE_E2E_AGENT_SOURCE: &str = "progressive-reviewer-e2e";
const GUIDE_E2E_AGENT_SESSION_SOURCE: &str = "herdr:codex";

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
    messages: &Receiver<Message>,
    expected_review_unit: &str,
    expected_checkpoint: &str,
    expected_text: &str,
) {
    loop {
        match messages.recv_timeout(Duration::from_secs(5)).unwrap() {
            Message::ReviewGuideLoaded {
                review_checkpoint,
                items,
            } => {
                assert!(review_checkpoint.matches(expected_review_unit, expected_checkpoint));
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].text, expected_text);
                return;
            }
            Message::ReviewGuideStatus {
                generating: false,
                message,
                ..
            } if message.is_some() => panic!("guide request failed: {message:?}"),
            _ => {}
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
    messages: Receiver<Message>,
    worker_thread: JoinHandle<()>,
    events: Receiver<HerdrEvent>,
    review_unit: String,
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
        let (message_sender, messages) = mpsc::channel();
        let worker_thread = thread::spawn(move || worker.run(&command_receiver, &message_sender));

        commands.send(WorkerCommand::Poll).unwrap();
        let (review_unit, checkpoint) = loop {
            if let Message::FilesLoaded {
                change_id,
                commit_id,
                ..
            } = messages.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                break (change_id, commit_id);
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

#[test]
fn finds_a_nested_rust_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let crates = directory.path().join("crates");
    std::fs::create_dir(&crates).unwrap();
    std::fs::write(crates.join("Cargo.toml"), "[workspace]\n").unwrap();

    assert_eq!(rust_project_root(directory.path()), Some(crates));
}

#[test]
fn only_rust_documents_are_opened_for_lsp() {
    assert_eq!(
        rust_document_path(std::path::Path::new("/repo"), "src/lib.rs"),
        Some(PathBuf::from("/repo/src/lib.rs"))
    );
    assert_eq!(
        rust_document_path(std::path::Path::new("/repo"), "README.md"),
        None
    );
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
        Some(Message::MouseScroll {
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
        Some(Message::MouseScroll {
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
            Some(Message::MouseClick {
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
        Some(Message::MouseControlClick { column: 4, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(Message::MouseDrag { column: 40, row: 5 })
    );
    assert_eq!(
        normalize_mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
        Some(Message::MouseRelease)
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
        Some(Message::MouseClick { .. })
    ));
    let adjacent = MouseEvent { column: 5, ..click };
    assert_eq!(
        clicks.normalize_at(adjacent, start + Duration::from_millis(400)),
        Some(Message::MouseDoubleClick { column: 5, row: 5 })
    );
    assert!(matches!(
        clicks.normalize_at(click, start + Duration::from_millis(450)),
        Some(Message::MouseClick { .. })
    ));
    let modified = MouseEvent {
        modifiers: KeyModifiers::CONTROL,
        ..click
    };
    clicks.normalize_at(modified, start + Duration::from_millis(460));
    assert!(matches!(
        clicks.normalize_at(click, start + Duration::from_millis(470)),
        Some(Message::MouseClick { .. })
    ));
}

#[test]
fn dispatch_reports_that_quit_stops_the_runtime() {
    let repository = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let settings = ReviewStore::open(state.path(), repository.path()).unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (commands, _command_receiver) = mpsc::channel();

    assert!(
        Runtime::dispatch(&commands, &settings, Action::Quit, repository.path(), &lsp,).unwrap()
    );
}

#[test]
fn worker_command_preserves_output_actions() {
    let repository = tempfile::tempdir().unwrap();
    let lsp = review_lsp::Worker::start(repository.path().to_owned());
    let (commands, _command_receiver) = mpsc::channel();

    let command = Runtime::worker_command(
        &commands,
        repository.path(),
        &lsp,
        Action::Output {
            target: OutputTarget::Clipboard,
            text: "selected code".to_owned(),
        },
    )
    .unwrap()
    .unwrap();

    assert!(matches!(
        command,
        WorkerCommand::Output {
            target: OutputTarget::Clipboard,
            text,
        } if text == "selected code"
    ));
}

#[test]
fn event_loop_runs_messages_until_quit_without_an_extra_cycle() {
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
    let mut app = ReviewApp::default();
    let (commands, _command_receiver) = mpsc::channel();
    let (message_sender, messages) = mpsc::channel();
    let (_herdr_event_sender, herdr_events) = mpsc::channel();
    let stopped = AtomicBool::new(false);
    message_sender.send(Message::Key(Key::Char('o'))).unwrap();
    message_sender.send(Message::Key(Key::Quit)).unwrap();
    drop(message_sender);

    RuntimeEventLoop {
        terminal: &mut terminal,
        app: &mut app,
        commands: &commands,
        messages: &messages,
        herdr_events: &herdr_events,
        lsp: &lsp,
        lsp_root: repository.path(),
        repository_root: repository.path(),
        settings: &settings,
        watcher: RepositoryWatcher::new(repository.path(), RepoType::Jj),
        mouse_clicks: MouseClicks::default(),
        stopped: &stopped,
    }
    .run()
    .unwrap();

    assert_eq!(settings.output_target().unwrap(), OutputTarget::Clipboard);
    std::mem::forget(terminal);
}
