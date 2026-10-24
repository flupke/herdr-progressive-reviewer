//! A Codex TUI in a Herdr test server, on an app server of its own with a scratch Codex home and
//! a local model fixture: the reviewer learns the agent's session from its call to a reviewer
//! tool, then follows the session through the daemon, across a `/resume` and a restart of the
//! daemon. It calls no model API.

use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use codex_daemon::{Daemon, NoDaemon, SessionEvent, SessionStatus, TurnEnd};
use herdr_client::protocol::AgentPort as _;
use review_mcp::{Endpoint, Response, Server};
use review_mcp_config::{Client, UserConfig};
use review_test_support::codex::{AppServer, CodexHome, Provider, prompt, wait_output};
use review_test_support::{GUARD, HerdrTestServer, TestPort};

#[test]
fn the_reviewer_follows_a_codex_agent_s_session_through_its_daemon() {
    let codex = CodexHome::new();
    let port = TestPort::new();
    let endpoint = Endpoint::for_repository(&codex.workspace(), Some(port.number())).unwrap();
    let (named, sessions) = mpsc::channel();
    let _reviewer = Server::start(endpoint, move |request| {
        let _ = named.send(request.agent_session.clone());
        request.respond(Ok(Response::Threads(Vec::new())));
        Ok(())
    })
    .unwrap();
    let provider = Provider::start(&codex, &bridge_registration(&codex));
    // MCP servers run in the daemon, which the bridge reaches the reviewer from.
    let bridge_port = port.number().to_string();
    let environment = [(
        "HERDR_REVIEWER_MCP_PORT",
        std::ffi::OsStr::new(&bridge_port),
    )];
    let mut daemon = Some(AppServer::start(&codex, &provider, &environment));
    let herdr = HerdrTestServer::start(&codex.workspace());
    let pane = codex.start_tui(&herdr, &["--remote", "unix://"]);

    prompt(&herdr, &pane, "call the reviewer");
    let session = sessions
        .recv_timeout(GUARD)
        .expect("the agent called no reviewer tool")
        .expect("the call named no agent session");
    let processes = herdr.client().pane_process_info(&pane).unwrap();
    let (heard, events) = mpsc::channel();
    let _follow = Daemon::of_pane(&processes.foreground_processes)
        .unwrap()
        .follow(&session, Duration::from_millis(20), move |event| {
            let _ = heard.send(event);
        });
    let mut events = Events(events);
    let joined = events.until("the join", |event| matches!(event, SessionEvent::Joined(_)));
    if joined.last() == Some(&SessionEvent::Joined(Some(SessionStatus::Active))) {
        events.until("the end of the first turn", |event| {
            matches!(event, SessionEvent::TurnEnded { .. })
        });
    }

    prompt(&herdr, &pane, "second turn");
    let heard = events.turn("second turn");
    assert!(
        !heard.contains(&SessionEvent::Resumed),
        "the reviewer's own join is no resume: {heard:?}"
    );

    prompt(&herdr, &pane, "/new");
    prompt(&herdr, &pane, &format!("/resume {session}"));
    let typed = Instant::now();
    events.until("the resume", |event| *event == SessionEvent::Resumed);
    assert!(
        typed.elapsed() < Duration::from_secs(1),
        "the resume was heard {:?} after it was typed",
        typed.elapsed()
    );

    prompt(&herdr, &pane, "after the resume");
    events.turn("after the resume");

    daemon.take().unwrap().stop();
    events.until("the loss of the daemon", |event| {
        matches!(event, SessionEvent::Lost(_))
    });
    wait_output(&herdr, &pane, "Reconnecting to server");
    daemon = Some(AppServer::start(&codex, &provider, &environment));
    let rejoined = events.until("the join again", |event| {
        matches!(event, SessionEvent::Joined(_))
    });
    assert_eq!(
        rejoined
            .iter()
            .filter(|event| matches!(event, SessionEvent::Lost(_)))
            .count(),
        0,
        "the loss is reported once: {rejoined:?}"
    );
    wait_output(&herdr, &pane, "? for shortcuts");
    prompt(&herdr, &pane, "after the restart");
    events.turn("after the restart");
    drop(daemon);
}

#[test]
fn a_codex_agent_without_the_daemon_has_none_to_join() {
    let codex = CodexHome::new();
    // The agent takes no turn: its provider listens nowhere.
    let registration = bridge_registration(&codex);
    codex.configure(9, &registration);
    let herdr = HerdrTestServer::start(&codex.workspace());
    let pane = codex.start_tui(&herdr, &["--no-daemon"]);

    let processes = herdr.client().pane_process_info(&pane).unwrap();

    assert_eq!(
        Daemon::of_pane(&processes.foreground_processes),
        Err(NoDaemon::WithoutDaemon)
    );
}

/// What the reviewer heard of the session, in order.
struct Events(mpsc::Receiver<SessionEvent>);

impl Events {
    /// The events up to the first one that `last` accepts, that one included.
    fn until(
        &mut self,
        what: &str,
        mut last: impl FnMut(&SessionEvent) -> bool,
    ) -> Vec<SessionEvent> {
        let mut heard = Vec::new();
        loop {
            let event = self
                .0
                .recv_timeout(GUARD)
                .unwrap_or_else(|_| panic!("the reviewer did not hear {what}, only {heard:?}"));
            let done = last(&event);
            heard.push(event);
            if done {
                return heard;
            }
        }
    }

    /// The events up to the end of the turn that starts on `prompt`, once they say that it
    /// started, took the prompt and completed, in that order.
    fn turn(&mut self, prompt: &str) -> Vec<SessionEvent> {
        let mut turn = None;
        let heard = self.until(&format!("the turn of {prompt:?}"), |event| match event {
            SessionEvent::Prompt { turn: of, text } if text == prompt => {
                turn = Some(of.clone());
                false
            }
            SessionEvent::TurnEnded { turn: ended, .. } => turn.as_ref() == Some(ended),
            _ => false,
        });
        let turn = turn.unwrap();
        let of_turn: Vec<_> = heard
            .iter()
            .filter(|event| match event {
                SessionEvent::TurnStarted { turn: of }
                | SessionEvent::Prompt { turn: of, .. }
                | SessionEvent::TurnEnded { turn: of, .. } => *of == turn,
                _ => false,
            })
            .cloned()
            .collect();
        assert_eq!(
            of_turn,
            [
                SessionEvent::TurnStarted { turn: turn.clone() },
                SessionEvent::Prompt {
                    turn: turn.clone(),
                    text: prompt.to_owned(),
                },
                SessionEvent::TurnEnded {
                    turn,
                    end: TurnEnd::Completed,
                },
            ]
        );
        heard
    }
}

/// The configuration that registers the reviewer's MCP bridge in `codex`'s home.
fn bridge_registration(codex: &CodexHome) -> String {
    assert!(
        UserConfig::new(
            Client::Codex,
            &codex.home(),
            env!("CARGO_BIN_EXE_reviewer-mcp").into()
        )
        .install()
        .unwrap()
    );
    let registration = fs::read_to_string(codex.home().join("config.toml")).unwrap();
    fs::remove_file(codex.home().join("config.toml")).unwrap();
    registration
}
