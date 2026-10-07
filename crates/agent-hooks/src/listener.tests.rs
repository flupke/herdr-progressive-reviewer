use std::path::Path;

use super::*;

/// How long a test waits for an event that a failing test never gets.
const GUARD: Duration = Duration::from_secs(30);

fn pane(id: &str) -> PaneId {
    PaneId(id.into())
}

fn started(pane: &str, session: &str, source: SessionSource) -> Report {
    Report {
        pane: self::pane(pane),
        event: AgentEvent::SessionStarted {
            session: session.into(),
            source,
        },
    }
}

fn submitted(pane: &str, prompt: &str) -> Report {
    Report {
        pane: self::pane(pane),
        event: AgentEvent::PromptSubmitted {
            session: "session".into(),
            prompt: prompt.into(),
        },
    }
}

/// A reviewer listening in a new directory.
fn reviewer() -> (tempfile::TempDir, HookDirectory, AgentHooks) {
    let runtime = tempfile::tempdir().unwrap();
    let directory = HookDirectory::for_server(runtime.path(), Path::new("/run/herdr.sock"));
    let hooks = AgentHooks::listen(&directory).unwrap();
    (runtime, directory, hooks)
}

#[test]
fn the_agent_resuming_the_expected_session_is_heard() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_resume(&pane("w:p1"), "fork", "blocked");

    directory.tell(&started("w:p1", "fork", SessionSource::Resume));

    assert_eq!(expectation.wait(GUARD), Some(Heard::Resumed));
}

#[test]
fn another_pane_session_or_start_is_not_what_an_expectation_waits_for() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_resume(&pane("w:p1"), "fork", "blocked");
    let marker = hooks.expect_resume(&pane("w:p9"), "marker", "blocked");

    directory.tell(&started("w:p2", "fork", SessionSource::Resume));
    directory.tell(&started("w:p1", "other", SessionSource::Resume));
    directory.tell(&started("w:p1", "fork", SessionSource::Startup));
    // The reviewer takes the hooks one after another: it took the events above before this.
    directory.tell(&started("w:p9", "marker", SessionSource::Resume));

    assert_eq!(marker.wait(GUARD), Some(Heard::Resumed));
    assert_eq!(expectation.wait(Duration::ZERO), None);
}

#[test]
fn a_reviewer_of_another_herdr_server_hears_nothing() {
    let (runtime, directory, hooks) = reviewer();
    let other = HookDirectory::for_server(runtime.path(), Path::new("/run/other-herdr.sock"));
    let expectation = hooks.expect_resume(&pane("w:p1"), "fork", "blocked");
    let marker = hooks.expect_resume(&pane("w:p9"), "marker", "blocked");

    other.tell(&started("w:p1", "fork", SessionSource::Resume));
    directory.tell(&started("w:p9", "marker", SessionSource::Resume));

    assert_eq!(marker.wait(GUARD), Some(Heard::Resumed));
    assert_eq!(expectation.wait(Duration::ZERO), None);
}

#[test]
fn a_new_reviewer_removes_the_sockets_of_stopped_ones_and_a_stopped_one_its_own() {
    let (_runtime, directory, hooks) = reviewer();
    let mut ended = std::process::Command::new("true").spawn().unwrap();
    ended.wait().unwrap();
    let stale = directory.socket(ended.id(), 0);
    fs::write(&stale, "").unwrap();

    let second = AgentHooks::listen(&directory).unwrap();

    let mut sockets = directory.sockets();
    sockets.sort();
    let mut listening = vec![hooks.socket.clone(), second.socket.clone()];
    listening.sort();
    assert_eq!(sockets, listening);
    drop(hooks);
    drop(second);
    assert_eq!(directory.sockets(), Vec::<PathBuf>::new());
}

#[test]
fn a_prompt_submitted_while_a_session_is_expected_is_blocked_with_its_reason() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_resume(&pane("w:p1"), "fork", "a draft met the /resume");

    let block = directory.ask(&submitted("w:p1", "half a thought /resume fork"), GUARD);

    assert_eq!(block.as_deref(), Some("a draft met the /resume"));
    assert_eq!(
        expectation.wait(Duration::ZERO),
        Some(Heard::Blocked {
            prompt: "half a thought /resume fork".into()
        })
    );
}

#[test]
fn a_prompt_goes_on_where_no_session_is_expected() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_resume(&pane("w:p1"), "fork", "blocked");

    assert_eq!(directory.ask(&submitted("w:p2", "hello"), GUARD), None);
    drop(expectation);
    assert_eq!(directory.ask(&submitted("w:p1", "hello"), GUARD), None);
}

#[test]
fn a_prompt_goes_on_when_no_reviewer_answers() {
    let runtime = tempfile::tempdir().unwrap();
    let directory = HookDirectory::for_server(runtime.path(), Path::new("/run/herdr.sock"));
    directory.create().unwrap();
    // The socket of a reviewer that stopped, and one of a reviewer that never answers.
    drop(crate::address::bind(&directory.socket(1, 0)).unwrap());
    let _silent = crate::address::bind(&directory.socket(2, 0)).unwrap();

    let block = directory.ask(&submitted("w:p1", "hello"), Duration::from_millis(1));

    assert_eq!(block, None);
}
