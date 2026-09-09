use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use ratatui::backend::TestBackend;
use review_repository::diff::DiffRow;
use ui_events::HighlightingFinished;

#[test]
fn file_selection_loads_on_the_next_tick_without_further_activity() {
    let root = tempfile::tempdir().unwrap();
    let mut scenario = Scenario::new(root.path().into());
    scenario.deliver(RepositoryFilesChanged {
        review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
        files: ["first.txt", "second.txt"]
            .into_iter()
            .map(|path| FileSummary::new(path, ReviewStatus::Unreviewed))
            .collect(),
    });
    scenario.pending_documents.try_iter().for_each(drop);
    scenario.deliver(UserInput::Key(Key::Down));
    assert!(scenario.pending_documents.try_recv().is_err());
    let inputs = scenario.interactive.clone();
    {
        let mut runtime = scenario.event_loop();
        inputs
            .send(EventEnvelope::new(ApplicationTick(Instant::now())))
            .unwrap();
        assert!(!runtime.cycle().unwrap());
        let frames = runtime.terminal.get_frame().count();
        inputs
            .send(EventEnvelope::new(ApplicationTick(Instant::now())))
            .unwrap();
        assert!(!runtime.cycle().unwrap());
        assert_eq!(runtime.terminal.get_frame().count(), frames);
    }
    let commands = scenario.pending_documents.try_iter().collect::<Vec<_>>();
    assert!(
        matches!(commands.as_slice(), [document::Command::LoadDiff { path, .. }] if path == "second.txt"),
        "{commands:?}"
    );
}

#[test]
fn idle_ticks_skip_frames_but_input_animation_and_toast_expiration_still_render() {
    let root = tempfile::tempdir().unwrap();
    let mut scenario = Scenario::new(root.path().into());
    let inputs = scenario.interactive.clone();
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

    let mut status = ui_events::ReviewGuideStatusChanged {
        review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
        generating: true,
        message: None,
    };
    runtime
        .dispatch_event(&EventEnvelope::new(status.clone()))
        .unwrap();
    inputs
        .send(EventEnvelope::new(ApplicationTick(Instant::now())))
        .unwrap();
    assert!(!runtime.cycle().unwrap());
    assert_eq!(runtime.terminal.get_frame().count(), frames + 2);

    status.generating = false;
    runtime.dispatch_event(&EventEnvelope::new(status)).unwrap();
    inputs
        .send(EventEnvelope::new(ApplicationTick(Instant::now())))
        .unwrap();
    assert!(!runtime.cycle().unwrap());
    assert_eq!(runtime.terminal.get_frame().count(), frames + 2);

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
    assert_eq!(runtime.terminal.get_frame().count(), frames + 4);
    assert!(
        !runtime
            .app
            .needs_tick(runtime.last_frame, Instant::now() + Duration::from_secs(5))
    );
}

struct DelayedHighlights {
    worker: highlighting::Worker,
    started: Receiver<()>,
    release: Sender<()>,
}

impl DelayedHighlights {
    fn new(events: EventSender<EventEnvelope>) -> Self {
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
        Self {
            worker,
            started: entered,
            release,
        }
    }
}

impl Drop for DelayedHighlights {
    fn drop(&mut self) {
        let _ = self.release.send(());
    }
}

struct Scenario {
    comments: comments::Worker,
    _state: tempfile::TempDir,
    root: PathBuf,
    settings: ReviewStore,
    terminal: Terminal<TestBackend>,
    app: ReviewApplication,
    lsp: review_lsp::Worker,
    highlighting: DelayedHighlights,
    search: text_search::Worker,
    commands: Sender<WorkerCommand>,
    _repository_commands: Receiver<WorkerCommand>,
    documents: Sender<document::Command>,
    pending_documents: Receiver<document::Command>,
    background: EventSender<EventEnvelope>,
    interactive: EventSender<EventEnvelope>,
    inbox: events::Inbox,
    timings: timing::Recorder,
}

impl Scenario {
    fn new(root: PathBuf) -> Self {
        let state = tempfile::tempdir().unwrap();
        let settings = ReviewStore::open(state.path(), &root).unwrap();
        let (background, events) = unbounded();
        let (interactive, inputs) = unbounded();
        let (commands, repository_commands) = mpsc::channel();
        let (documents, document_commands) = mpsc::channel();
        let results = interactive.clone();
        let search = text_search::Worker::start(move |result| {
            let _ = results.send(EventEnvelope::new(result));
        });
        Self {
            comments: comment_service::test_worker(&settings),
            _state: state,
            app: ReviewApplication::new(Theme::default(), None, root.clone()),
            lsp: review_lsp::Worker::start(root.clone()),
            root,
            settings,
            terminal: Terminal::new(TestBackend::new(100, 20)).unwrap(),
            highlighting: DelayedHighlights::new(background.clone()),
            search,
            commands,
            _repository_commands: repository_commands,
            documents,
            pending_documents: document_commands,
            background,
            interactive,
            inbox: events::Inbox::new(events, inputs),
            timings: timing::Recorder::from_env().unwrap(),
        }
    }

    fn event_loop(&mut self) -> RuntimeEventLoop<'_, TestBackend> {
        RuntimeEventLoop {
            target: AgentTarget::new(herdr_client::protocol::WorkspaceId("test".into()), None),
            source_watches: None,
            last_frame: Instant::now(),
            comments: &self.comments,
            terminal: &mut self.terminal,
            app: &mut self.app,
            commands: &self.commands,
            documents: &self.documents,
            search: &self.search,
            highlighting: &self.highlighting.worker,
            events: &mut self.inbox,
            timings: &self.timings,
            lsp: &self.lsp,
            repository_root: &self.root,
            settings: &self.settings,
        }
    }

    fn deliver(&mut self, event: impl ApplicationEvent) {
        assert!(
            !self
                .event_loop()
                .handle_event(&EventEnvelope::new(event))
                .unwrap()
        );
    }

    fn prepare(&mut self) {
        self.deliver(RepositoryFilesChanged {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            files: vec![FileSummary::new("source.rs", ReviewStatus::Unreviewed)],
        });
        let lines = ["prefix needle".to_owned(), "second needle".to_owned()]
            .into_iter()
            .chain((0..1_000).map(|_| format!("// {}", "x".repeat(80))))
            .collect::<Vec<_>>();
        self.deliver(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "source.rs".to_owned(),
            rows: lines
                .iter()
                .enumerate()
                .map(|(index, line)| DiffRow::Add {
                    new_line: u32::try_from(index + 1).unwrap(),
                    text: format!("+{line}"),
                })
                .collect(),
            old_content: None,
            new_content: Some(format!("{}\n", lines.join("\n")).into_bytes()),
        });
        let startup = self
            .lsp
            .event_receiver()
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
        });
        for _ in 0..5_000 {
            self.background.send(stale.clone()).unwrap();
        }
    }

    fn key(&self, key: Key) {
        self.interactive
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
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.screen().contains(text) {
            assert!(
                Instant::now() < deadline,
                "missing {text}: {}",
                self.screen()
            );
            self.background
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
            !self.background.is_empty(),
            "a frame must be drawn before the backlog drains"
        );
        self.wait_for_screen("[1/2]");
        assert!(
            !self.background.is_empty(),
            "search results must get through the backlog"
        );
        self.key(Key::Enter);
        self.key(Key::Char('n'));
        self.wait_for_screen("[2/2]");
        assert!(
            self.lsp.try_recv().is_none(),
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
    fs::write(directory.path().join("source.rs"), "fn real_file() {}\n").unwrap();
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
    assert!(
        samples
            .iter()
            .all(|sample| sample["queued_ms"].as_f64().unwrap() >= 0.0
                && sample["work_ms"].as_f64().unwrap() >= 0.0)
    );
}

#[test]
#[ignore = "runs in a child process with a private language-server launcher"]
fn stalled_startup_process() {
    let root = env::var_os("HERDR_RESPONSIVENESS_TEST").expect("parent test supplies the fixture");
    Scenario::new(root.into()).check_search();
}
