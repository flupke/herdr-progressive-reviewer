//! The hc backend against a real herdr-core, on a private core of its own: its own
//! `HC_SESSION` and scratch XDG folders, never the user's core. Opt-in, since the dev shell
//! pins no herdr-core: `cargo nextest run -p hc-client --run-ignored only`, with `hc` on the
//! `PATH` or `TEST_HC_BIN` naming one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use hc_client::HcClient;
use herdr_client::client::EventCanceller;
use herdr_client::protocol::{AgentHost, AgentPort, AgentStatus, HerdrEvent, PaneId, TabId};
use serde_json::{Value, json};

const SESSION: &str = "t";

/// A herdr-core started for one test, and stopped with it.
struct PrivateCore {
    /// Short, under `/tmp`: the socket's path must fit in 107 bytes.
    runtime: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl PrivateCore {
    fn start() -> Self {
        let core = Self {
            runtime: tempfile::Builder::new()
                .prefix("hc")
                .tempdir_in("/tmp")
                .unwrap(),
            home: tempfile::tempdir().unwrap(),
        };
        core.hc(&["server", "start", "--json"]);
        core
    }

    fn hc(&self, arguments: &[&str]) -> String {
        let binary = std::env::var_os("TEST_HC_BIN").unwrap_or_else(|| OsString::from("hc"));
        let folder = |name: &str| self.home.path().join(name);
        let output = Command::new(binary)
            .args(arguments)
            .env_remove("HC_TERMINAL_ID")
            .env_remove("HC_TERMINAL_SOCKET")
            .env_remove("HC_SCOPE")
            .env("HC_SESSION", SESSION)
            .env("XDG_RUNTIME_DIR", self.runtime.path())
            .env("XDG_STATE_HOME", folder("state"))
            .env("XDG_CONFIG_HOME", folder("config"))
            .env("XDG_DATA_HOME", folder("data"))
            .env("XDG_CACHE_HOME", folder("cache"))
            .output()
            .unwrap();
        assert!(output.status.success(), "hc {arguments:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn socket(&self) -> PathBuf {
        self.runtime
            .path()
            .join("herdr-core")
            .join(format!("{SESSION}.sock"))
    }

    /// Waits until herdr-core names the foreground process of `terminal`, which it tells only
    /// in its raw events.
    fn wait_for_foreground(&self, terminal: &PaneId) {
        let mut socket = UnixStream::connect(self.socket()).unwrap();
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "events.subscribe",
            "params": {"terminal": terminal.0},
        });
        writeln!(socket, "{request}").unwrap();
        for line in BufReader::new(socket).lines() {
            let message: Value = serde_json::from_str(&line.unwrap()).unwrap();
            let named = !message["result"]["terminals"][0]["foreground"].is_null()
                || message["params"]["type"] == "foreground";
            if named {
                return;
            }
        }
        panic!("herdr-core closed the stream before it named the foreground process");
    }

    fn client(&self) -> HcClient {
        HcClient::new(self.socket())
    }

    /// Runs `script` in a new terminal of the tab `tb1`, in `folder`; returns its address.
    fn run(&self, folder: &Path, script: &str) -> PaneId {
        let started: Value = serde_json::from_str(&self.hc(&[
            "run",
            "--bg",
            "--json",
            "--cwd",
            folder.to_str().unwrap(),
            "--label",
            "herdr.space=sp1",
            "--label",
            "herdr.tab=tb1",
            "--",
            "bash",
            "-c",
            script,
        ]))
        .unwrap();
        PaneId(started["address"].as_str().unwrap().to_owned())
    }
}

impl Drop for PrivateCore {
    fn drop(&mut self) {
        self.hc(&["server", "stop", "--json"]);
    }
}

/// A shell line that reports `state` as Claude Code does, with OSC 7501.
fn report(state: &str) -> String {
    format!(r"printf '\033]7501;app=claude-code:state={state}\007';")
}

/// Waits until the agent of `pane` reports `status`, and returns the statuses until then.
fn wait_for(client: &HcClient, pane: &PaneId, status: AgentStatus) -> Vec<AgentStatus> {
    let mut seen = Vec::new();
    client
        .forward_agent_status(pane, &EventCanceller::default(), &mut |reported| {
            seen.push(reported);
            reported != status
        })
        .unwrap();
    seen
}

#[test]
#[ignore = "needs herdr-core: hc on the PATH or TEST_HC_BIN"]
fn the_agents_of_a_tab_are_its_terminals_that_report_a_status() {
    let core = PrivateCore::start();
    let folder = tempfile::tempdir().unwrap();
    let client = core.client();
    let agent = core.run(folder.path(), &format!("{} read -r line", report("idle")));
    let shell = core.run(folder.path(), "exec sleep 600");
    wait_for(&client, &agent, AgentStatus::Idle);

    let agents = client.list_agents().unwrap();

    let [listed] = agents.as_slice() else {
        panic!("one agent: {agents:?}");
    };
    assert_eq!(listed.pane_id, agent);
    assert_eq!(listed.tab_id, TabId("tb1".into()));
    assert_eq!(listed.agent.as_deref(), Some("claude"));
    assert_eq!(listed.agent_status, AgentStatus::Idle);
    assert_eq!(client.get_agent(&shell).unwrap(), None);
    assert_eq!(client.get_agent(&PaneId("t999".into())).unwrap(), None);
    // herdr-core names the foreground process a moment after the program starts.
    core.wait_for_foreground(&agent);
    let processes = client.pane_process_info(&agent).unwrap();
    assert_eq!(
        processes
            .foreground_processes
            .iter()
            .map(|process| (Some(process.pid), process.name.as_str()))
            .collect::<Vec<_>>(),
        [(processes.foreground_process_group_id, "bash")]
    );
}

#[test]
#[ignore = "needs herdr-core: hc on the PATH or TEST_HC_BIN"]
fn a_prompt_reaches_the_agent_as_typed_and_the_agent_starts_on_it() {
    let core = PrivateCore::start();
    let folder = tempfile::tempdir().unwrap();
    let client = core.client();
    let agent = core.run(
        folder.path(),
        &format!(
            "{} read -r line; printf '%s' \"$line\" > prompt; {} exec sleep 600",
            report("idle"),
            report("working")
        ),
    );
    wait_for(&client, &agent, AgentStatus::Idle);

    client
        .prompt_agent(&agent, "Review crates/hc-client")
        .unwrap();

    // The agent wrote the prompt before it went working, which the send waited for.
    assert_eq!(
        std::fs::read_to_string(folder.path().join("prompt")).unwrap(),
        "Review crates/hc-client"
    );
}

#[test]
#[ignore = "needs herdr-core: hc on the PATH or TEST_HC_BIN"]
fn an_agent_that_never_starts_on_a_prompt_is_told() {
    let core = PrivateCore::start();
    let folder = tempfile::tempdir().unwrap();
    let client = core.client();
    let agent = core.run(folder.path(), &format!("{} exec cat", report("idle")));
    wait_for(&client, &agent, AgentStatus::Idle);

    // herdr-core's own window of a few seconds after the send tells that `cat` never worked.
    assert!(matches!(
        client.prompt_agent(&agent, "Review crates/hc-client"),
        Err(herdr_client::Error::AgentNotStarted { .. })
    ));
}

#[test]
#[ignore = "needs herdr-core: hc on the PATH or TEST_HC_BIN"]
fn an_agents_statuses_follow_its_turns_and_end_with_its_program() {
    let core = PrivateCore::start();
    let folder = tempfile::tempdir().unwrap();
    let client = core.client();
    let agent = core.run(
        folder.path(),
        &format!(
            "{} read -r line; {} {} read -r line",
            report("idle"),
            report("working"),
            report("done")
        ),
    );
    wait_for(&client, &agent, AgentStatus::Idle);
    let (statuses, seen) = mpsc::channel();
    let watcher = {
        let (client, agent) = (client.clone(), agent.clone());
        thread::spawn(move || {
            client
                .forward_agent_status(&agent, &EventCanceller::default(), &mut |status| {
                    statuses.send(status).is_ok()
                })
                .unwrap();
        })
    };
    assert_eq!(seen.recv().unwrap(), AgentStatus::Idle);

    client.submit_prompt(&agent, "go").unwrap();
    assert_eq!(seen.recv().unwrap(), AgentStatus::Working);
    assert_eq!(seen.recv().unwrap(), AgentStatus::Done);
    client.submit_prompt(&agent, "exit").unwrap();

    // The program exits: its status clears, and the stream ends.
    watcher.join().unwrap();
    assert_eq!(seen.try_iter().collect::<Vec<_>>(), [AgentStatus::Unknown]);
}

#[test]
#[ignore = "needs herdr-core: hc on the PATH or TEST_HC_BIN"]
fn an_agent_that_exits_leaves_and_labels_reach_its_terminal() {
    let core = PrivateCore::start();
    let folder = tempfile::tempdir().unwrap();
    let client = core.client();
    let agent = core.run(folder.path(), &format!("{} read -r line", report("idle")));
    wait_for(&client, &agent, AgentStatus::Idle);
    let (events, told) = mpsc::channel();
    {
        let client = client.clone();
        thread::spawn(move || {
            client.forward_events(&EventCanceller::default(), &mut |event| {
                events.send(event).is_ok()
            })
        });
    }

    client
        .set_labels(
            &agent,
            &BTreeMap::from([("review.mark".to_owned(), "1".to_owned())]),
        )
        .unwrap();
    assert_eq!(
        client
            .labels(&agent)
            .unwrap()
            .get("review.mark")
            .map(String::as_str),
        Some("1")
    );
    // An agent that arrives after the stream started tells that the subscription holds.
    let marker = core.run(folder.path(), &format!("{} read -r line", report("idle")));
    assert!(matches!(
        told.recv().unwrap(),
        HerdrEvent::AgentDetected { pane_id, released: false, .. } if pane_id == marker
    ));
    client.submit_prompt(&agent, "exit").unwrap();

    assert_eq!(
        told.recv().unwrap(),
        HerdrEvent::AgentDetected {
            pane_id: agent,
            workspace_id: herdr_client::protocol::WorkspaceId("sp1".into()),
            agent: Some("claude".into()),
            released: true,
            final_status: Some(AgentStatus::Idle),
        }
    );
}
