//! The live viewer in a real terminal: what a person watching a vision
//! session sees, types and gets back when they leave.

use std::io::Write;
use std::os::unix::net::UnixListener;
use std::thread;
use std::time::Duration;

use review_test_support::eventually;

use tui_test::{KeyAction, OpenOptions, Operation, OperationResult, RunOptions, Session, Timeouts};

fn screen(session: &Session) -> String {
    let OperationResult::Text(text) = session.execute(Operation::Text { full: false }).unwrap()
    else {
        panic!("expected the screen text");
    };
    text
}

fn mode(session: &Session, name: &str) -> bool {
    let OperationResult::Modes(modes) = session.execute(Operation::GetModes).unwrap() else {
        panic!("expected terminal modes");
    };
    modes[name]
}

/// Wait up to five seconds for the screen to show `text`.
fn shows(session: &Session, text: &str) -> bool {
    eventually(Duration::from_secs(5), || screen(session).contains(text))
}

fn press(session: &Session, key: &str) {
    session
        .execute(Operation::Key {
            keys: vec![key.into()],
            action: KeyAction::Press,
        })
        .unwrap();
}

#[test]
fn the_viewer_shows_the_stream_ignores_keys_and_restores_the_terminal_on_q() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("stream.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let session = Session::new(format!("live-viewer-{}", std::process::id()));
    let defaults = OpenOptions::default();
    // The shell reports the keyboard settings the viewer leaves behind.
    session
        .run(RunOptions {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                r#""$0" --view "$1"; echo "exit $?"; stty -a"#.into(),
                env!("CARGO_BIN_EXE_reviewer-vision").into(),
                socket.to_str().unwrap().into(),
            ],
            cwd: None,
            env: Vec::new(),
            cols: 100,
            rows: 30,
            wait_ready: Some(false),
            backend: defaults.backend,
            profile: defaults.profile,
            restart: false,
            timeouts: Timeouts::default(),
            recording: tui_test::AutomaticRecording::default(),
        })
        .unwrap();
    let (mut stream, _) = listener.accept().unwrap();

    stream.write_all(b"\x1b[Hstreamed frame").unwrap();
    assert!(shows(&session, "streamed frame"));
    assert!(mode(&session, "alternate_screen"));
    assert!(!mode(&session, "cursor_visible"));
    assert!(!mode(&session, "wraparound"));

    press(&session, "Q");
    thread::sleep(Duration::from_millis(200));
    assert!(!screen(&session).contains('Q'), "a typed key is echoed");

    press(&session, "q");
    assert!(shows(&session, "exit 0"));
    assert!(!mode(&session, "alternate_screen"));
    assert!(mode(&session, "cursor_visible"));
    assert!(mode(&session, "wraparound"));
    let settings = screen(&session);
    assert!(settings.contains(" echo ") && settings.contains(" icanon "));
}
