use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};

use effects::fixture::EffectsFixture;
use herdr_client::protocol::HerdrEvent;
use ratatui::backend::TestBackend;
use review_repository::diff::DiffRow;
use review_repository::repository::RepoType;
use review_source::ReviewCheckpoint;
use ui_events::{DiffContentLoadFailed, DiffContentLoaded, HighlightingFinished};

#[test]
fn file_selection_loads_on_the_next_tick_without_further_activity() {
    let mut scenario = Scenario::new();
    scenario.fixture.files.write("first.txt", b"first file\n");
    scenario.fixture.files.write("second.txt", b"second file\n");
    scenario.fixture.effects.refresh().unwrap();
    scenario.wait_for_repository_screen("first file");

    scenario.deliver(UserInput::Key(Key::Down));
    let pending = scenario.pending_events(Duration::from_millis(200));
    assert!(
        !pending.iter().any(|event| {
            event.downcast_ref::<DiffContentLoaded>().is_some()
                || event.downcast_ref::<DiffContentLoadFailed>().is_some()
        }),
        "moving the selection alone loads nothing"
    );
    for event in pending {
        scenario.fixture.background.send(event).unwrap();
    }

    scenario.wait_for_repository_screen("second file");
}

#[test]
fn idle_ticks_skip_frames_but_input_and_toast_expiration_still_render() {
    let mut scenario = Scenario::new();
    let inputs = scenario.fixture.interactive.clone();
    let mut runtime = scenario.event_loop();
    runtime.redraw().unwrap();
    let frames = runtime.terminal.get_frame().count();
    for _ in 0..100 {
        inputs
            .send(EventEnvelope::new(ApplicationTick(Instant::now())))
            .unwrap();
        assert!(!runtime.cycle().unwrap());
    }
    assert_eq!(runtime.terminal.get_frame().count(), frames);

    inputs
        .send(EventEnvelope::new(UserInput::Key(Key::Char('t'))))
        .unwrap();
    assert!(!runtime.cycle().unwrap());
    assert_eq!(runtime.terminal.get_frame().count(), frames + 1);

    runtime
        .dispatch_event(&EventEnvelope::new(ui_events::ToastRequested {
            text: "Saved".into(),
            kind: toasts::ToastKind::Info,
        }))
        .unwrap();
    runtime.redraw().unwrap();
    inputs
        .send(EventEnvelope::new(ApplicationTick(
            Instant::now() + Duration::from_secs(4),
        )))
        .unwrap();
    assert!(!runtime.cycle().unwrap());
    assert_eq!(runtime.terminal.get_frame().count(), frames + 3);
    assert!(
        !runtime
            .app
            .needs_tick(runtime.last_frame, Instant::now() + Duration::from_secs(5))
    );
}

#[test]
fn agent_detection_events_and_batched_idle_ticks_do_not_repaint() {
    let mut scenario = Scenario::new();
    let events = scenario.fixture.background.clone();
    let mut runtime = scenario.event_loop();
    runtime.redraw().unwrap();
    let frames = runtime.terminal.get_frame().count();
    for released in [false, true] {
        events
            .send(EventEnvelope::new(HerdrEvent::AgentDetected {
                pane_id: PaneId("companion".into()),
                workspace_id: WorkspaceId("test".into()),
                agent: Some("codex".into()),
                released,
                final_status: None,
            }))
            .unwrap();
        events
            .send(EventEnvelope::new(ApplicationTick(Instant::now())))
            .unwrap();
        assert!(!runtime.cycle().unwrap());
        assert_eq!(runtime.terminal.get_frame().count(), frames);
    }
    events
        .send(EventEnvelope::new(HerdrEvent::AgentDetected {
            pane_id: PaneId("companion".into()),
            workspace_id: WorkspaceId("test".into()),
            agent: Some("codex".into()),
            released: false,
            final_status: None,
        }))
        .unwrap();
    events
        .send(EventEnvelope::new(UserInput::Key(Key::Char('t'))))
        .unwrap();
    assert!(!runtime.cycle().unwrap());
    assert_eq!(runtime.terminal.get_frame().count(), frames + 1);
}

/// Highlight results wait until the test releases them.
struct DelayedHighlights {
    started: Receiver<()>,
    release: Sender<()>,
}

impl DelayedHighlights {
    fn start(events: EventSender<EventEnvelope>) -> (Self, highlighting::Worker) {
        let (started, entered) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let theme = Theme::default();
        let worker = highlighting::Worker::start(
            syntax_highlighting::SyntaxHighlighter::new(theme.syntax, theme.palette.text),
            move |result: HighlightingFinished| {
                let _ = started.send(());
                let _ = gate.recv();
                let _ = events.send(EventEnvelope::new(result));
            },
        );
        (
            Self {
                started: entered,
                release,
            },
            worker,
        )
    }
}

impl Drop for DelayedHighlights {
    fn drop(&mut self) {
        let _ = self.release.send(());
    }
}

struct Scenario {
    // Declared first so stalled highlights are released before the workers stop.
    highlighting: DelayedHighlights,
    fixture: EffectsFixture,
    terminal: Terminal<TestBackend>,
    app: ReviewApplication,
    timings: timing::Recorder,
}

impl Scenario {
    fn new() -> Self {
        let mut fixture = EffectsFixture::new(RepoType::Git);
        let (highlighting, worker) = DelayedHighlights::start(fixture.background.clone());
        fixture.effects.replace_highlighting(worker);
        let root = fixture.repository.root().to_owned();
        Self {
            highlighting,
            fixture,
            terminal: Terminal::new(TestBackend::new(100, 20)).unwrap(),
            app: ReviewApplication::new(Theme::default(), None, root),
            timings: timing::Recorder::from_env().unwrap(),
        }
    }

    fn event_loop(&mut self) -> RuntimeEventLoop<'_, TestBackend> {
        RuntimeEventLoop {
            effects: &mut self.fixture.effects,
            terminal_events: None,
            terminal: &mut self.terminal,
            app: &mut self.app,
            events: &mut self.fixture.inbox,
            timings: &self.timings,
            last_frame: Instant::now(),
        }
    }

    fn deliver(&mut self, event: impl component_core::ApplicationEvent) {
        let flow = self
            .event_loop()
            .handle_event(&EventEnvelope::new(event))
            .unwrap();
        assert!(flow.is_continue());
    }

    /// Events that arrive within `timeout`, taken before the event loop sees them.
    fn pending_events(&mut self, timeout: Duration) -> Vec<EventEnvelope> {
        let deadline = Instant::now() + timeout;
        std::iter::from_fn(|| {
            self.fixture
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        })
        .collect()
    }

    fn prepare(&mut self) {
        let lines = ["prefix needle".to_owned(), "second needle".to_owned()]
            .into_iter()
            .chain((0..1_000).map(|_| format!("// {}", "x".repeat(80))))
            .collect::<Vec<_>>();
        self.fixture
            .files
            .write("source.rs", format!("{}\n", lines.join("\n")).as_bytes());
        self.fixture.effects.refresh().unwrap();
        self.wait_for_repository_screen("prefix needle");
        let startup = self
            .fixture
            .effects
            .lsp_events()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(matches!(startup, review_lsp::Event::Initializing(_)));
        self.deliver(startup);
        self.highlighting
            .started
            // Cold syntax initialization is setup, not part of the measured
            // input/frame latency. Allow it to finish on a loaded test host.
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
    }

    fn flood_background(&self) {
        // Old loads still in flight when a revision changes must not starve input.
        let stale = EventEnvelope::new(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("old change", "old checkpoint"),
            path: "source.rs".to_owned(),
            rows: vec![DiffRow::Add {
                new_line: 1,
                text: "+obsolete".to_owned(),
            }],
            old_content: None,
            new_content: None,
            hunks: review_hunks::FileHunks::default(),
        });
        for _ in 0..5_000 {
            self.fixture.background.send(stale.clone()).unwrap();
        }
    }

    fn key(&self, key: Key) {
        self.fixture
            .interactive
            .send(EventEnvelope::new(UserInput::Key(key)))
            .unwrap();
    }

    fn screen(&self) -> String {
        self.terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    fn wait_for_screen(&mut self, text: &str) {
        self.wait_for_screen_within(text, Duration::from_secs(2));
    }

    /// Wait for repository work, which is setup rather than measured latency.
    fn wait_for_repository_screen(&mut self, text: &str) {
        self.wait_for_screen_within(text, Duration::from_secs(10));
    }

    fn wait_for_screen_within(&mut self, text: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.screen().contains(text) {
            assert!(
                Instant::now() < deadline,
                "missing {text}: {}",
                self.screen()
            );
            self.fixture
                .background
                .send(EventEnvelope::new(ApplicationTick(Instant::now())))
                .unwrap();
            assert!(!self.event_loop().cycle().unwrap());
        }
    }

    fn check_search(&mut self) {
        self.prepare();
        self.flood_background();
        self.key(Key::Tab);
        self.key(Key::Char('/'));
        for character in "needle".chars() {
            self.key(Key::Char(character));
        }
        self.wait_for_screen("/needle");
        assert!(
            !self.fixture.background.is_empty(),
            "a frame must be drawn before the backlog drains"
        );
        self.wait_for_screen("[1/2]");
        assert!(
            !self.fixture.background.is_empty(),
            "search results must get through the backlog"
        );
        self.key(Key::Enter);
        self.key(Key::Char('n'));
        self.wait_for_screen("[2/2]");
        assert!(
            self.fixture.effects.lsp_events().is_empty(),
            "the language server must still be starting"
        );
        assert!(
            self.highlighting.started.try_recv().is_err(),
            "highlight delivery remains blocked"
        );
    }
}

#[test]
fn search_and_frames_progress_while_lsp_startup_and_highlights_are_stalled() {
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let direnv = bin.join("direnv");
    fs::write(&direnv, "#!/bin/sh\nexec sleep 30\n").unwrap();
    fs::set_permissions(&direnv, fs::Permissions::from_mode(0o755)).unwrap();
    let inherited_path = env::var_os("PATH").unwrap_or_default();
    let path =
        env::join_paths(std::iter::once(bin).chain(env::split_paths(&inherited_path))).unwrap();
    let mut child = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "runtime::responsiveness::stalled_startup_process",
            "--ignored",
            "--nocapture",
        ])
        .env("PATH", path)
        .env("HERDR_RESPONSIVENESS_TEST", directory.path())
        .env(
            "HERDR_REVIEWER_TIMINGS",
            directory.path().join("timings.jsonl"),
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let samples = fs::read_to_string(directory.path().join("timings.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(samples.iter().any(|sample| sample["event"] == "frame"));
    assert!(samples.iter().any(|sample| {
        sample["event"]
            .as_str()
            .is_some_and(|name| name.ends_with("UserInput"))
    }));
    assert!(samples.iter().all(
        |sample| sample["queued_ms"].as_f64().is_some() && sample["work_ms"].as_f64().is_some()
    ));
}

#[test]
#[ignore = "runs in a child process with a private language-server launcher"]
fn stalled_startup_process() {
    env::var_os("HERDR_RESPONSIVENESS_TEST").expect("parent test supplies the launcher");
    Scenario::new().check_search();
}
