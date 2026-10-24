use std::path::Path;

use herdr_client::protocol::Agent;

use super::*;
use crate::HookRoot;

/// How long a test waits for an event that a failing test never gets.
const GUARD: Duration = Duration::from_secs(30);

fn pane(id: &str) -> PaneId {
    PaneId(id.into())
}

/// The Claude Code agent of `pane`, on the session `session`, as its hooks name it.
fn in_pane(pane: &str) -> HookedAgent {
    HookedAgent::Pane {
        pane: self::pane(pane),
        session: Some("session".into()),
    }
}

fn started(pane: &str, session: &str, source: SessionSource) -> Report {
    Report {
        pane: Some(self::pane(pane)),
        event: AgentEvent::SessionStarted {
            session: session.into(),
            source,
        },
    }
}

fn submitted(pane: &str, prompt: &str) -> Report {
    submitted_on(pane, prompt, Some("session"))
}

fn submitted_on(pane: &str, prompt: &str, session: Option<&str>) -> Report {
    Report {
        pane: Some(self::pane(pane)),
        event: AgentEvent::PromptSubmitted {
            prompt: prompt.into(),
            session: session.map(Into::into),
        },
    }
}

/// A prompt as the reviewer types it, its line ended.
const TYPED: &str = "Explore round r1: read /tmp/turn/index.md and follow it.\n";

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
    let expectation = hooks.expect_resume(&in_pane("w:p1"), "fork", "blocked");

    directory.tell(&started("w:p1", "fork", SessionSource::Resume));

    assert_eq!(expectation.wait(GUARD), Some(Heard::Resumed));
}

#[test]
fn another_pane_session_or_start_is_not_what_an_expectation_waits_for() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_resume(&in_pane("w:p1"), "fork", "blocked");
    let marker = hooks.expect_resume(&in_pane("w:p9"), "marker", "blocked");

    directory.tell(&started("w:p2", "fork", SessionSource::Resume));
    directory.tell(&started("w:p1", "other", SessionSource::Resume));
    directory.tell(&started("w:p1", "fork", SessionSource::Other));
    // The reviewer takes the hooks one after another: it took the events above before this.
    directory.tell(&started("w:p9", "marker", SessionSource::Resume));

    assert_eq!(marker.wait(GUARD), Some(Heard::Resumed));
    assert_eq!(expectation.wait(Duration::ZERO), None);
}

#[test]
fn a_reviewer_of_another_herdr_server_hears_nothing() {
    let (runtime, directory, hooks) = reviewer();
    let other = HookDirectory::for_server(runtime.path(), Path::new("/run/other-herdr.sock"));
    let expectation = hooks.expect_resume(&in_pane("w:p1"), "fork", "blocked");
    let marker = hooks.expect_resume(&in_pane("w:p9"), "marker", "blocked");

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
    let expectation =
        hooks.expect_resume(&in_pane("w:p1"), "fork", "text in the box met the /resume");

    let block = directory.ask(&submitted("w:p1", "half a thought /resume fork"), GUARD);

    assert_eq!(block.as_deref(), Some("text in the box met the /resume"));
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
    let expectation = hooks.expect_resume(&in_pane("w:p1"), "fork", "blocked");

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

#[test]
fn each_process_of_a_pane_keeps_the_last_session_it_started_and_one_that_ended_goes() {
    let runtime = tempfile::tempdir().unwrap();
    let directory = HookDirectory::for_server(runtime.path(), Path::new("/run/herdr.sock"));
    let this = agent_fork::ProcessStamp::read(std::process::id());
    let mut ended = std::process::Command::new("true").spawn().unwrap();
    ended.wait().unwrap();
    let ended = agent_fork::ProcessStamp {
        pid: ended.id(),
        started: 7,
    };
    let started = |session: &str, process| HookedSession {
        session: session.into(),
        process,
    };

    directory
        .record(&pane("w:p1"), &started("gone", ended))
        .unwrap();
    directory
        .record(&pane("w/p2"), &started("other", this))
        .unwrap();
    directory
        .record(&pane("w:p1"), &started("first", this))
        .unwrap();
    directory
        .record(&pane("w:p1"), &started("resumed", this))
        .unwrap();

    assert_eq!(
        directory.hooked_sessions(&pane("w:p1")),
        [started("resumed", this)]
    );
    assert_eq!(
        directory.hooked_sessions(&pane("w/p2")),
        [started("other", this)]
    );
    assert_eq!(directory.hooked_sessions(&pane("w:p3")), []);
}

#[test]
fn the_prompt_the_reviewer_typed_goes_on_and_is_heard() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_prompt(&in_pane("w:p1"), TYPED, "blocked");

    // The agent trims the line's end.
    let block = directory.ask(&submitted("w:p1", TYPED.trim_end()), GUARD);

    assert_eq!(block, None);
    assert_eq!(expectation.wait(GUARD), Some(Heard::Submitted));
}

#[test]
fn text_left_in_the_input_box_that_joined_the_typed_prompt_is_blocked_with_its_reason() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation =
        hooks.expect_prompt(&in_pane("w:p1"), TYPED, "text in the box met the prompt");
    let joined = format!("half a thought{}", TYPED.trim_end());

    let block = directory.ask(&submitted("w:p1", &joined), GUARD);

    assert_eq!(block.as_deref(), Some("text in the box met the prompt"));
    assert_eq!(
        expectation.wait(Duration::ZERO),
        Some(Heard::Joined {
            left: "half a thought".into()
        })
    );
}

#[test]
fn the_typed_prompt_on_another_session_is_stopped_even_with_text_left_in_the_box() {
    let (_runtime, directory, hooks) = reviewer();
    let alone = hooks.expect_prompt(&in_pane("w:p1"), TYPED, "blocked");
    let joined = hooks.expect_prompt(&in_pane("w:p2"), TYPED, "blocked");

    let block = directory.ask(&submitted_on("w:p1", TYPED, Some("nested")), GUARD);
    let joined_block = directory.ask(
        &submitted_on("w:p2", &format!("half a thought{TYPED}"), Some("nested")),
        GUARD,
    );

    assert_eq!(block.as_deref(), Some("blocked"));
    assert_eq!(joined_block.as_deref(), Some("blocked"));
    assert_eq!(alone.wait(Duration::ZERO), Some(Heard::Stopped));
    assert_eq!(joined.wait(Duration::ZERO), Some(Heard::Stopped));
}

#[test]
fn a_prompt_without_the_typed_one_is_blocked_and_the_reviewer_still_waits_for_its_own() {
    let (_runtime, directory, hooks) = reviewer();
    let expectation = hooks.expect_prompt(&in_pane("w:p1"), TYPED, "blocked");

    let other = directory.ask(&submitted("w:p1", "hello"), GUARD);
    assert_eq!(other.as_deref(), Some("blocked"));
    assert_eq!(expectation.wait(Duration::ZERO), None);
    // A hook older than the session field reports none.
    let own = directory.ask(&submitted_on("w:p1", TYPED, None), GUARD);

    assert_eq!(own, None);
    assert_eq!(expectation.wait(Duration::ZERO), Some(Heard::Submitted));
    drop(expectation);
    assert_eq!(directory.ask(&submitted("w:p1", "hello"), GUARD), None);
}

#[test]
fn a_prompt_and_a_resume_expected_in_one_pane_each_hear_what_they_wait_for() {
    let (_runtime, directory, hooks) = reviewer();
    let resume = hooks.expect_resume(&in_pane("w:p1"), "fork", "resuming");
    let prompt = hooks.expect_prompt(&in_pane("w:p1"), TYPED, "typing");

    // While the agent resumes a session, even the typed prompt is blocked.
    let block = directory.ask(&submitted("w:p1", TYPED), GUARD);

    assert_eq!(block.as_deref(), Some("resuming"));
    let blocked = Some(Heard::Blocked {
        prompt: TYPED.into(),
    });
    assert_eq!(prompt.wait(Duration::ZERO), Some(Heard::Stopped));
    assert_eq!(resume.wait(Duration::ZERO), blocked);
    drop(prompt);
    let later = hooks.expect_resume(&in_pane("w:p1"), "later", "resuming");
    directory.tell(&started("w:p1", "later", SessionSource::Resume));
    assert_eq!(later.wait(GUARD), Some(Heard::Resumed));
    assert_eq!(resume.wait(Duration::ZERO), blocked);
}

/// The Codex agent on the session `session`, as its hooks name it.
fn codex(session: &str) -> HookedAgent {
    HookedAgent::Codex {
        session: session.into(),
    }
}

/// A prompt submitted to a Codex agent, as its hook reports it: by session, with no pane.
fn codex_submitted(session: &str, prompt: &str) -> Report {
    Report {
        pane: None,
        event: AgentEvent::PromptSubmitted {
            prompt: prompt.into(),
            session: Some(session.into()),
        },
    }
}

#[test]
fn a_codex_hook_reaches_the_reviewer_of_any_herdr_server_that_expects_the_session() {
    let (runtime, _directory, _hooks) = reviewer();
    let other = HookDirectory::for_server(runtime.path(), Path::new("/run/other-herdr.sock"));
    let hooks = AgentHooks::listen(&other).unwrap();
    let alone = hooks.expect_prompt(&codex("thread-1"), TYPED, "blocked");
    let joined = hooks.expect_prompt(&codex("thread-2"), TYPED, "text in the box met the prompt");
    let root = HookRoot::new(runtime.path());

    let alone_block = root.ask(&codex_submitted("thread-1", TYPED.trim_end()), GUARD);
    let joined_block = root.ask(
        &codex_submitted("thread-2", &format!("half a thought{}", TYPED.trim_end())),
        GUARD,
    );

    assert_eq!(alone_block, None);
    assert_eq!(alone.wait(GUARD), Some(Heard::Submitted));
    assert_eq!(
        joined_block.as_deref(),
        Some("text in the box met the prompt")
    );
    assert_eq!(
        joined.wait(Duration::ZERO),
        Some(Heard::Joined {
            left: "half a thought".into()
        })
    );
}

#[test]
fn a_codex_prompt_goes_on_on_a_session_or_a_pane_that_expects_nothing() {
    let (runtime, _directory, hooks) = reviewer();
    let codex_resume = hooks.expect_resume(&codex("thread-1"), "fork", "blocked");
    // A Claude Code pane whose ID is the session's has nothing to do with it.
    let pane = hooks.expect_resume(&in_pane("thread-2"), "fork", "blocked");
    let root = HookRoot::new(runtime.path());

    assert_eq!(root.ask(&codex_submitted("thread-2", "hello"), GUARD), None);
    assert_eq!(pane.wait(Duration::ZERO), None);
    assert_eq!(
        root.ask(
            &codex_submitted("thread-1", "half a thought/resume fork"),
            GUARD
        )
        .as_deref(),
        Some("blocked")
    );
    assert_eq!(
        codex_resume.wait(Duration::ZERO),
        Some(Heard::Blocked {
            prompt: "half a thought/resume fork".into()
        })
    );
}

/// An agent of Herdr's in the pane `pane`, named `name`, with the session `session` if any.
fn herdr_agent(pane: &str, name: &str, session: Option<&str>) -> Agent {
    serde_json::from_value(serde_json::json!({
        "pane_id": pane,
        "tab_id": "tab",
        "workspace_id": "workspace",
        "agent": name,
        "agent_status": "idle",
        "agent_session": session.map(|value| serde_json::json!({
            "agent": name, "kind": "session_id", "value": value, "source": "hook"
        })),
    }))
    .unwrap()
}

#[test]
fn a_codex_agent_runs_the_hooks_once_its_hook_recorded_the_session_it_named() {
    let (runtime, _directory, hooks) = reviewer();
    let port = herdr_client::memory::InMemoryAgents::default();
    // Herdr's session for a Codex pane can be another pane's: the reviewer never reads it.
    let agent = herdr_agent("w:p1", "codex", Some("herdr-session"));
    let root = HookRoot::new(runtime.path());
    root.record_codex_session("herdr-session").unwrap();

    assert_eq!(
        hooks.hooked(&port, &agent),
        None,
        "the agent named no session"
    );
    hooks.name_session(&pane("w:p1"), "thread-1");
    assert_eq!(
        hooks.hooked(&port, &agent),
        None,
        "the hook never ran there"
    );
    root.record_codex_session("thread-1").unwrap();
    assert_eq!(hooks.hooked(&port, &agent), Some(codex("thread-1")));
    hooks.name_session(&pane("w:p1"), "thread-2");
    assert_eq!(hooks.hooked(&port, &agent), None, "the agent moved on");
    assert_eq!(
        hooks.hooked(&port, &herdr_agent("w:p2", "codex", None)),
        None,
        "another pane's agent named nothing"
    );
}

#[test]
fn a_codex_fork_s_session_is_never_named_the_pane_s() {
    let (runtime, _directory, hooks) = reviewer();
    let root = HookRoot::new(runtime.path());
    hooks.name_session(&pane("w:p1"), "thread-1");

    root.mark_codex_fork("fork-1").unwrap();
    hooks.name_session(&pane("w:p1"), "fork-1");
    assert_eq!(
        hooks.named_session(&pane("w:p1")).as_deref(),
        Some("thread-1")
    );
    assert!(root.is_codex_fork("fork-1"));

    // Once the agent may run it, the fork's session is the agent's.
    root.unmark_codex_fork("fork-1").unwrap();
    hooks.name_session(&pane("w:p1"), "fork-1");
    assert_eq!(
        hooks.named_session(&pane("w:p1")).as_deref(),
        Some("fork-1")
    );
    assert!(!root.is_codex_fork("fork-1"));
    root.unmark_codex_fork("fork-1").unwrap();
}
