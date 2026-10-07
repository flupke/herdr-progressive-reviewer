//! Sessions in a private Herdr, with the reviewer that `make e2e-tui` builds. Every wait ends on
//! an event of Herdr's: the guard fires only when a test fails.

use review_test_support::HerdrTestServer;
use serde_json::Value;

use super::super::workspace::Closing;

use super::*;

const GUARD: Duration = Duration::from_secs(30);

struct Fixture {
    session: Session,
    herdr: HerdrClient,
    // Stop the server after the session closed its workspace.
    _server: HerdrTestServer,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn start() -> (Self, Screen) {
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
            },
            directory.path().join("session"),
        )
        .unwrap();
        let fixture = Self {
            session,
            herdr,
            _server: server,
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
        _server,
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
        _server,
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
    session.reopen().unwrap();
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
