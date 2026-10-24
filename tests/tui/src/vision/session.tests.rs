//! Sessions in a private Herdr, with the reviewer that `make e2e-tui` builds. Every wait ends on
//! an event of Herdr's: the guard fires only when a test fails.

use review_test_support::HerdrTestServer;
use review_test_support::stand_in::Reported;
use serde_json::Value;

use super::super::workspace::Closing;

use super::*;

const GUARD: Duration = Duration::from_secs(30);

struct Fixture {
    session: Session,
    herdr: HerdrClient,
    // Stop the server after the session closed its workspace.
    server: HerdrTestServer,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn start() -> (Self, Screen) {
        Self::start_with(true)
    }

    /// A session whose stand-in agent runs the reviewer's plugin hooks with `agent_hooks`.
    fn start_with(agent_hooks: bool) -> (Self, Screen) {
        let directory = tempfile::tempdir().unwrap();
        let server = HerdrTestServer::start(directory.path());
        let herdr = server.client();
        let reviewer = std::env::var_os("REVIEWER_BIN_PATH")
            .expect("run with make e2e-tui, which builds the reviewer");
        let (session, screen) = Session::start(
            &herdr,
            reviewer.into(),
            &StartOptions {
                repository: RepoType::Jj,
                seed: Seed::sample(),
                focus: false,
                agent_hooks,
            },
            directory.path().join("session"),
        )
        .unwrap();
        let fixture = Self {
            session,
            herdr,
            server,
            _directory: directory,
        };
        (fixture, screen)
    }

    fn workspaces(&self) -> Vec<String> {
        self.herdr.request("workspace.list", &json!({})).unwrap()["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|workspace| workspace["workspace_id"].as_str().unwrap().to_owned())
            .collect()
    }
}

fn keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn each_action_returns_the_screen_of_the_reviewers_reaction() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    let files = session.wait_for("math.rs", false, GUARD).unwrap();

    // The help shows in the screen the key returns, with no wait of its own.
    let help = session.act(&Input::Keys(&keys(&["?"]))).unwrap();
    assert!(help.shows("Keyboard shortcuts"), "{}", help.text);
    assert!(help.frame > files.frame);
    let closed = session.act(&Input::Keys(&keys(&["esc"]))).unwrap();
    assert!(!closed.shows("Keyboard shortcuts"), "{}", closed.text);

    let math = closed.locate("math.rs").unwrap();
    session.act(&Input::Click(math)).unwrap();
    let diff = session.wait_for("left - right", false, GUARD).unwrap();
    assert!(diff.shows("Diff · src/math.rs"), "{}", diff.text);
}

#[test]
fn mistakes_are_errors() {
    let (mut fixture, screen) = Fixture::start();
    let session = &mut fixture.session;
    session.wait_for("math.rs", false, GUARD).unwrap();
    let unknown = session
        .act(&Input::Keys(&keys(&["?", "bogus"])))
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("unsupported key bogus"), "{unknown}");
    // The acknowledgement comes after anything the refused call could have written.
    let after = session.settle().unwrap();
    assert!(!after.shows("Keyboard shortcuts"), "{}", after.text);
    let outside = Cell {
        x: screen.columns,
        y: 0,
    };
    assert!(session.act(&Input::Click(outside)).is_err());
    let absent = session
        .wait_for("no such text", false, Duration::ZERO)
        .unwrap_err()
        .to_string();
    assert!(
        absent.contains("does not show \"no such text\""),
        "{absent}"
    );
}

#[test]
fn the_stand_in_agent_answers_an_explore_round_in_the_pane() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    session.wait_for("math.rs", false, GUARD).unwrap();
    let explore = session.screen(false).unwrap().locate("Explore").unwrap();
    let start = session.act(&Input::Click(explore)).unwrap();
    let in_the_pane = start.locate("Start in the pane").unwrap();
    session.act(&Input::Click(in_the_pane)).unwrap();

    let kickoff = session.next_turn(None, GUARD).unwrap();
    assert_eq!(kickoff["kind"], "kickoff");
    let question: Value =
        serde_json::from_str(include_str!("../../examples/question.json")).unwrap();
    let (reply, screen) = session
        .reply(1, "submit_question", question["arguments"].clone())
        .unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
    assert!(screen.shows("Question 1"), "{}", screen.text);
}

#[test]
fn a_stopped_session_closes_its_workspace_and_removes_its_repository() {
    let (fixture, _) = Fixture::start();
    let described = fixture.session.describe();
    let workspace = described["workspace"].as_str().unwrap().to_owned();
    let repository = described["repository"].as_str().unwrap().to_owned();
    assert!(fixture.workspaces().contains(&workspace));
    let Fixture {
        session,
        herdr,
        server: _server,
        _directory,
    } = fixture;
    assert_eq!(session.stop(), Closing::Closed);
    let remaining = herdr.request("workspace.list", &json!({})).unwrap();
    assert!(!remaining.to_string().contains(&format!("\"{workspace}\"")));
    assert!(!std::path::Path::new(&repository).exists());
}

#[test]
fn a_workspace_holding_a_pane_the_session_did_not_open_stays_open() {
    let (fixture, _) = Fixture::start();
    let described = fixture.session.describe();
    let workspace = described["workspace"].as_str().unwrap().to_owned();
    // The user's own pane, beside the stand-in agent.
    let split = fixture
        .herdr
        .request(
            "pane.split",
            &json!({"target_pane_id": described["agent_pane"], "direction": "down"}),
        )
        .unwrap();
    let foreign = split["pane"]["pane_id"].as_str().unwrap().to_owned();
    let Fixture {
        session,
        herdr,
        server: _server,
        _directory,
    } = fixture;
    assert_eq!(
        session.stop(),
        Closing::LeftOpen(format!(
            "it holds pane {foreign}, which the session did not open"
        ))
    );
    let remaining = herdr.request("workspace.list", &json!({})).unwrap();
    assert!(remaining.to_string().contains(&format!("\"{workspace}\"")));
}

#[test]
fn a_reopened_reviewer_starts_once_the_old_one_has_stopped() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    session.wait_for("math.rs", false, GUARD).unwrap();
    let before = session.describe()["reviewer_pane"].clone();
    session.reopen(None).unwrap();
    assert_ne!(session.describe()["reviewer_pane"], before);
    session.wait_for("math.rs", false, GUARD).unwrap();
    // The new reviewer serves the MCP port the old one held: Explore reaches its agent.
    let explore = session.screen(false).unwrap().locate("Explore").unwrap();
    let start = session.act(&Input::Click(explore)).unwrap();
    let in_the_pane = start.locate("Start in the pane").unwrap();
    session.act(&Input::Click(in_the_pane)).unwrap();
    session.next_turn(None, GUARD).unwrap();
    let question: Value =
        serde_json::from_str(include_str!("../../examples/question.json")).unwrap();
    let (reply, _) = session
        .reply(1, "submit_question", question["arguments"].clone())
        .unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
}

#[test]
fn a_reviewer_process_that_already_ended_counts_as_stopped() {
    let mut ended = std::process::Command::new("true").spawn().unwrap();
    ended.wait().unwrap();
    terminate(ended.id()).unwrap();
}

#[test]
fn a_running_reviewer_process_is_asked_to_stop() {
    use std::os::unix::process::ExitStatusExt;

    let mut running = std::process::Command::new("sleep")
        .arg("600")
        .spawn()
        .unwrap();
    if let Err(error) = terminate(running.id()) {
        running.kill().unwrap();
        panic!("{error}");
    }
    // SIGTERM is 15 on Linux and macOS.
    assert_eq!(running.wait().unwrap().signal(), Some(15));
}

/// The JSON a reviewer MCP tool returned, from the text of its result.
fn tool_json(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn the_stand_in_agent_answers_a_review_comment_in_the_threads_tab() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    session.wait_for("math.rs", false, GUARD).unwrap();
    for key in ["tab", "j", "j", "a"] {
        session.act(&Input::Keys(&[key.to_owned()])).unwrap();
    }
    session.act(&Input::Text("Why this line?")).unwrap();
    session
        .act(&Input::Keys(&["ctrl+enter".to_owned()]))
        .unwrap();

    let wakeup = session.next_turn(None, GUARD).unwrap();
    assert_eq!(wakeup["kind"], "comments", "{wakeup}");
    let turn = wakeup["turn"].as_u64().unwrap();
    let (fetched, _) = session
        .reply(turn, "get_new_messages", serde_json::json!({}))
        .unwrap();
    let thread = &tool_json(&fetched)["threads"][0];
    let (replied, _) = session
        .reply(
            turn,
            "reply",
            serde_json::json!({
                "thread_id": thread["thread_id"],
                "in_reply_to": thread["in_reply_to"],
                "message_id": "6f1c2a9e-3b7d-4e5a-9c8f-1d2e3f4a5b6c",
                "text": "Because the fixture needs a third line.",
            }),
        )
        .unwrap();
    assert_eq!(replied["isError"], false, "{replied}");

    session.act(&Input::Keys(&["t".to_owned()])).unwrap();
    let screen = session
        .wait_for("Because the fixture needs a third line.", false, GUARD)
        .unwrap();
    assert!(screen.shows("Why this line?"), "{}", screen.text);
}

#[test]
fn the_stand_in_agent_revises_the_open_question_in_the_pane() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    session.wait_for("math.rs", false, GUARD).unwrap();
    let explore = session.screen(false).unwrap().locate("Explore").unwrap();
    let start = session.act(&Input::Click(explore)).unwrap();
    let in_the_pane = start.locate("Start in the pane").unwrap();
    session.act(&Input::Click(in_the_pane)).unwrap();
    session.next_turn(None, GUARD).unwrap();
    let question: Value =
        serde_json::from_str(include_str!("../../examples/question.json")).unwrap();
    session
        .reply(1, "submit_question", question["arguments"].clone())
        .unwrap();
    let mut revised = question["arguments"]["update"]["next"].clone();
    revised["version"] = 2.into();
    revised["text"] = "Should the helpers reject negative input?".into();

    let (reply, _) = session
        .reply(
            1,
            "revise_question",
            serde_json::json!({"revision": {
                "message": "message-1", "reason": "You said only positive input reaches them.",
                "question": revised,
            }}),
        )
        .unwrap();

    assert_eq!(reply["isError"], false, "{reply}");
    let screen = session
        .wait_for("Should the helpers reject negative input?", false, GUARD)
        .unwrap();
    assert!(
        screen.shows("Revised after your message"),
        "{}",
        screen.text
    );
}

#[test]
fn the_driver_plays_a_fork_and_a_bare_answer_continues_as_it() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    session.wait_for("math.rs", false, GUARD).unwrap();
    let explore = session.screen(false).unwrap().locate("Explore").unwrap();
    session.act(&Input::Click(explore)).unwrap();
    let every = session.act(&Input::Keys(&keys(&["z", "z"]))).unwrap();
    assert!(
        every.shows("Run ahead (experimental): every choice"),
        "{}",
        every.text
    );
    session
        .act(&Input::Click(every.locate("Start in the pane").unwrap()))
        .unwrap();
    session.next_turn(None, GUARD).unwrap();
    let question: Value =
        serde_json::from_str(include_str!("../../examples/question.json")).unwrap();
    session
        .reply(1, "submit_question", question["arguments"].clone())
        .unwrap();

    let forks = session.forks(Some(2), GUARD).unwrap();
    let choices: Vec<_> = forks["forks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|fork| fork["choice"].as_str().unwrap())
        .collect();
    assert_eq!(choices, ["keep", "saturate"], "{forks}");
    let own = forks["agent"]["session"].clone();
    let keep = forks["forks"][0]["session"].as_str().unwrap().to_owned();
    assert_eq!(forks["forks"][0]["parent"], own);
    let conclusion: Value =
        serde_json::from_str(include_str!("../../examples/conclusion.json")).unwrap();
    // The reviewer hears the agent's hooks in the session's runtime directory, never in the one
    // its panes inherit, which stands for the user's.
    let hooks = |runtime: &Path| runtime.join("herdr-reviewer").exists();
    assert!(hooks(Path::new(
        session.describe()["runtime"].as_str().unwrap()
    )));
    let inherited = &fixture.server.environment()[std::ffi::OsStr::new("XDG_RUNTIME_DIR")];
    assert!(!hooks(Path::new(inherited)));
    let session = &mut fixture.session;
    let (reply, _) = session
        .fork_submit(&keep, "submit_conclusion", conclusion["arguments"].clone())
        .unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
    let shown = session.forks(None, GUARD).unwrap();
    assert_eq!(shown["forks"][0]["submitted"], true, "{shown}");

    // A bare answer: the first choice, sent with no comment.
    session
        .act(&Input::Keys(&keys(&["1", "ctrl+enter"])))
        .unwrap();

    let prepared = session
        .wait_for("Prepared while you were thinking", false, GUARD)
        .unwrap();
    assert!(prepared.shows("To be implemented"), "{}", prepared.text);
    // The agent tells the reviewer and Herdr of its new session before the event socket.
    session
        .events
        .try_wait_until("the agent on the fork's session", GUARD, |received| {
            forks::agent_sessions(received).session.as_deref() == Some(&keep)
        })
        .unwrap();
    let moved = session.forks(None, GUARD).unwrap();
    assert_eq!(moved["agent"]["session"], keep.as_str(), "{moved}");
    assert_eq!(moved["agent"]["resumed"], serde_json::json!([keep]));
}

/// Starts a round in the pane with run-ahead on every choice, and asks the example question.
fn ask_with_run_ahead(session: &mut Session) {
    session.wait_for("math.rs", false, GUARD).unwrap();
    let explore = session.screen(false).unwrap().locate("Explore").unwrap();
    session.act(&Input::Click(explore)).unwrap();
    let every = session.act(&Input::Keys(&keys(&["z", "z"]))).unwrap();
    session
        .act(&Input::Click(every.locate("Start in the pane").unwrap()))
        .unwrap();
    let kickoff = session.next_turn(None, GUARD).unwrap();
    let question: Value =
        serde_json::from_str(include_str!("../../examples/question.json")).unwrap();
    session
        .reply(
            kickoff["turn"].as_u64().unwrap(),
            "submit_question",
            question["arguments"].clone(),
        )
        .unwrap();
}

/// The forks of the example question, by choice: `keep`, then `saturate`.
fn forks_of_the_question(session: &Session) -> (String, String) {
    let forks = session.forks(Some(2), GUARD).unwrap();
    let session_of = |index: usize| {
        forks["forks"][index]["session"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    (session_of(0), session_of(1))
}

/// Answers the first choice with no comment, then answers the agent's turn with the next
/// question, and returns the screen that shows it with the line of the turn's path.
fn answer_bare_and_ask_again(session: &mut Session) -> Screen {
    session
        .act(&Input::Keys(&keys(&["1", "ctrl+enter"])))
        .unwrap();
    ask_again(session)
}

/// Answers the agent's turn after an answer with the next question, and returns the screen
/// that shows it with the line of the turn's path.
fn ask_again(session: &mut Session) -> Screen {
    let wakeup = session.next_turn(None, GUARD).unwrap();
    assert_eq!(wakeup["kind"], "wakeup", "{wakeup}");
    let question: Value =
        serde_json::from_str(include_str!("../../examples/question.json")).unwrap();
    let mut arguments = question["arguments"].clone();
    let update = &mut arguments["update"];
    update["design"] = Value::Null;
    update["topics"][0]["id"] = "rest".into();
    update["next"]["id"] = "q2".into();
    update["next"]["topic"] = "rest".into();
    update["next"]["text"] = "Should the helpers check their input?".into();
    update["interpretation"] =
        serde_json::json!({"status": "accepted", "recap": "Kept.", "follow_ups": []});
    update["reply"] = serde_json::json!({"text": "Kept as written.", "evidence": []});
    let (reply, _) = session
        .reply(
            wakeup["turn"].as_u64().unwrap(),
            "submit_question",
            arguments,
        )
        .unwrap();
    assert_eq!(reply["isError"], false, "{reply}");
    session
        .wait_for("Should the helpers check their input?", false, GUARD)
        .unwrap()
}

#[test]
fn the_driver_ends_one_fork_without_a_turn_and_crashes_the_other() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    ask_with_run_ahead(session);
    let (keep, saturate) = forks_of_the_question(session);

    session.fork_end(&keep, false).unwrap();
    session.fork_end(&saturate, true).unwrap();

    let forks = session.forks(None, GUARD).unwrap();
    let ended = |index: usize| {
        let fork = &forks["forks"][index];
        (
            fork["submitted"].clone(),
            fork["finished"].clone(),
            fork["exited"].clone(),
        )
    };
    assert_eq!(
        ended(0),
        (false.into(), true.into(), true.into()),
        "{forks}"
    );
    assert_eq!(
        ended(1),
        (false.into(), false.into(), true.into()),
        "{forks}"
    );
    let log = forks["log"].as_str().unwrap();
    assert!(log.contains("exit status: 1"), "{log}");
    let screen = answer_bare_and_ask_again(session);
    assert!(
        screen.shows("Not prepared: the turn for this choice did not finish"),
        "{}",
        screen.text
    );
}

#[test]
fn a_switch_heard_late_still_continues_as_the_fork() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    ask_with_run_ahead(session);
    let (keep, _) = forks_of_the_question(session);
    let conclusion: Value =
        serde_json::from_str(include_str!("../../examples/conclusion.json")).unwrap();
    session
        .fork_submit(&keep, "submit_conclusion", conclusion["arguments"].clone())
        .unwrap();
    session
        .switch_unheard(Some(Duration::from_secs(2)))
        .unwrap();

    session
        .act(&Input::Keys(&keys(&["1", "ctrl+enter"])))
        .unwrap();

    // The agent reads the /resume at once, and reports it to Herdr and the hooks 2 s later.
    let from_agent = |received: &[Reported], event: &StandInEvent| {
        received
            .iter()
            .any(|reported| reported.from == StandInRole::Agent && reported.event == *event)
    };
    let read = StandInEvent::ResumeReceived {
        session: keep.clone(),
    };
    session
        .events
        .try_wait_until("the agent's /resume", GUARD, |received| {
            from_agent(received, &read)
        })
        .unwrap();
    let read_at = std::time::Instant::now();
    let reported = StandInEvent::SessionReported {
        session: keep.clone(),
        start: "resume".into(),
    };
    session
        .events
        .try_wait_until("the resume's report", GUARD, |received| {
            from_agent(received, &reported)
        })
        .unwrap();
    assert!(read_at.elapsed() >= Duration::from_secs(1));
    session
        .wait_for("Prepared while you were thinking", false, GUARD)
        .unwrap();
}

#[test]
fn an_unheard_switch_fails_and_retry_reaches_the_agent_once() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    ask_with_run_ahead(session);
    let (keep, _) = forks_of_the_question(session);
    let conclusion: Value =
        serde_json::from_str(include_str!("../../examples/conclusion.json")).unwrap();
    session
        .fork_submit(&keep, "submit_conclusion", conclusion["arguments"].clone())
        .unwrap();
    session.switch_unheard(None).unwrap();
    session
        .act(&Input::Keys(&keys(&["1", "ctrl+enter"])))
        .unwrap();

    // The switch waits 20 s for the agent's hook, fails, then sends the agent back to its own
    // session: the turn waits for Retry by then.
    let own = session.forks(None, GUARD).unwrap()["agent"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    session
        .events
        .try_wait_until(
            "the agent back on its session",
            Duration::from_secs(40),
            |received| {
                forks::agent_sessions(received).session.as_deref() == Some(own.as_str())
                    && received.iter().any(|reported| {
                        reported.event
                            == StandInEvent::ResumeReceived {
                                session: own.clone(),
                            }
                    })
            },
        )
        .unwrap();
    session.act(&Input::Keys(&keys(&["r"]))).unwrap();

    let screen = ask_again(session);
    assert!(
        screen.shows("Not prepared: the agent could not switch to the prepared turn"),
        "{}",
        screen.text
    );
    // The next turn the agent gets answers the next question: Retry sent the first answer once.
    session.act(&Input::Keys(&keys(&["1"]))).unwrap();
    session.act(&Input::Text("With a comment.")).unwrap();
    session.act(&Input::Keys(&keys(&["ctrl+enter"]))).unwrap();
    let next = session.next_turn(None, GUARD).unwrap();
    assert_eq!(next["answer"]["question"]["id"], "q2", "{next}");
    let sessions = session.forks(None, GUARD).unwrap()["agent"].clone();
    assert_eq!(
        sessions["resumed"],
        serde_json::json!([keep, own]),
        "{sessions}"
    );
}

#[test]
fn an_agent_shown_working_at_the_answer_takes_it_itself() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    ask_with_run_ahead(session);
    forks_of_the_question(session);

    session.agent_working(true).unwrap();
    session
        .act(&Input::Keys(&keys(&["1", "ctrl+enter"])))
        .unwrap();
    session.agent_working(false).unwrap();

    let screen = ask_again(session);
    assert!(
        screen.shows("Not prepared: the agent was working"),
        "{}",
        screen.text
    );
}

#[test]
fn a_prompt_typed_to_the_agent_moves_its_conversation_past_the_forks() {
    let (mut fixture, _) = Fixture::start();
    let session = &mut fixture.session;
    ask_with_run_ahead(session);
    forks_of_the_question(session);
    // The agent writes the prompt into its conversation and starts no turn on it: the answer
    // finds it idle, its conversation past the forks, whatever the answer's pace.
    let swallow = session.scratch.stand_in().swallow();
    fs::write(&swallow, "").unwrap();

    session.agent_prompt("What are you doing?").unwrap();
    fs::remove_file(&swallow).unwrap();

    let screen = answer_bare_and_ask_again(session);
    assert!(
        screen.shows("Not prepared: the agent's conversation moved on"),
        "{}",
        screen.text
    );
}

#[test]
fn an_agent_without_the_plugin_hooks_is_never_forked() {
    let (mut fixture, _) = Fixture::start_with(false);
    let session = &mut fixture.session;
    ask_with_run_ahead(session);

    let screen = answer_bare_and_ask_again(session);

    assert!(
        screen.shows("Not prepared: restart the agent to load the reviewer's Claude Code plugin"),
        "{}",
        screen.text
    );
    assert_eq!(
        session.forks(None, GUARD).unwrap()["forks"],
        serde_json::json!([])
    );
}
