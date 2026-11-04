use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use herdr_client::protocol::WorkspaceId;
use serde_json::{Value, json};

use super::*;

/// A stand-in for herdr-core's socket: each connection reads one request, hands it to the test,
/// and gets the lines `answer` gives for it, then stays open until the client closes it.
struct FakeCore {
    _directory: tempfile::TempDir,
    requests: Receiver<Value>,
}

fn fake_core(
    answer: impl Fn(&Value) -> Vec<Value> + Send + Sync + 'static,
) -> (HcClient, FakeCore) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("core.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let (requests, received) = mpsc::channel();
    let answer = std::sync::Arc::new(answer);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let (requests, answer) = (requests.clone(), std::sync::Arc::clone(&answer));
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                let lines = answer(&request);
                let _ = requests.send(request);
                for line in lines {
                    let _ = writeln!(stream, "{line}");
                }
                let _ = reader.read_to_end(&mut Vec::new());
            });
        }
    });
    (
        HcClient::new(path),
        FakeCore {
            _directory: directory,
            requests: received,
        },
    )
}

fn result(request: &Value, result: Value) -> Value {
    let mut answer = json!({"jsonrpc": "2.0", "id": request["id"]});
    answer["result"] = result;
    answer
}

fn refusal_of(request: &Value, kind: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": request["id"],
        "error": {"code": -32000, "message": format!("refused: {kind}"), "data": {"kind": kind}},
    })
}

fn notification(method: &str, params: Value) -> Value {
    let mut notification = json!({"jsonrpc": "2.0", "method": method});
    notification["params"] = params;
    notification
}

fn event(kind: &str, data: Value) -> Value {
    event_of("t3", kind, data)
}

fn event_of(terminal: &str, kind: &str, data: Value) -> Value {
    let mut params = json!({"type": kind, "terminal": terminal, "seq": 2});
    params["data"] = data;
    notification("event", params)
}

/// The agent `t3` of the tab `tb1`, with the OSC 7501 `state` its program reported.
fn agent_terminal(state: &str) -> Value {
    json!({
        "address": "t3",
        "argv": ["claude"],
        "cols": 80,
        "rows": 24,
        "created_ms": 1,
        "cwd": "/src",
        "labels": {"herdr.space": "sp1", "herdr.tab": "tb1"},
        "screen": "exact",
        "state": "live",
        "status": {"app": "claude-code", "state": state},
    })
}

fn pane() -> PaneId {
    PaneId("t3".into())
}

#[test]
fn a_terminal_herdr_core_does_not_know_holds_no_agent() {
    let (client, _core) = fake_core(|request| vec![refusal_of(request, "not_found")]);

    assert_eq!(client.get_agent(&pane()).unwrap(), None);
}

#[test]
fn another_refusal_is_an_error() {
    let (client, _core) = fake_core(|request| vec![refusal_of(request, "machine_unknown")]);

    assert!(matches!(
        client.get_agent(&pane()),
        Err(Error::Herdr {
            operation: "terminal.get",
            ..
        })
    ));
}

#[test]
fn a_prompt_is_sent_as_typed_without_a_caller_to_tag_it() {
    let (client, core) =
        fake_core(|request| vec![result(request, json!({"started": true, "turn": 1}))]);

    client.prompt_agent(&pane(), "Review x.rs").unwrap();

    let request = core.requests.recv().unwrap();
    assert_eq!(request["method"], "terminal.send");
    assert_eq!(
        request["params"],
        json!({"terminal": "t3", "text": "Review x.rs"})
    );
}

#[test]
fn an_agent_that_stays_idle_after_a_prompt_did_not_start_on_it() {
    let (client, _core) = fake_core(|request| match request["method"].as_str() {
        Some("terminal.send") => vec![result(
            request,
            json!({"started": false, "status": "idle", "turn": 1}),
        )],
        _ => vec![result(request, agent_terminal("idle"))],
    });

    assert!(matches!(
        client.prompt_agent(&pane(), "Review x.rs"),
        Err(Error::AgentNotStarted { .. })
    ));
}

#[test]
fn an_agent_that_blocked_on_its_user_after_a_prompt_started_on_it() {
    let (client, _core) = fake_core(|request| match request["method"].as_str() {
        Some("terminal.send") => vec![result(
            request,
            json!({"started": false, "status": "idle", "turn": 1}),
        )],
        _ => vec![result(request, agent_terminal("blocked"))],
    });

    client.prompt_agent(&pane(), "Review x.rs").unwrap();
}

#[test]
fn a_submitted_command_does_not_wait_for_the_agent_to_start() {
    let (client, core) = fake_core(|request| match request["method"].as_str() {
        Some("terminal.send") => vec![result(
            request,
            json!({"started": false, "status": "idle", "turn": 1}),
        )],
        _ => vec![result(request, agent_terminal("idle"))],
    });

    client.submit_prompt(&pane(), "/resume s1").unwrap();

    let methods: Vec<Value> = core
        .requests
        .try_iter()
        .map(|request| request["method"].clone())
        .collect();
    assert_eq!(methods, [json!("terminal.send")]);
}

#[test]
fn the_agents_of_the_tabs_are_the_terminals_that_report_a_status() {
    let (client, _core) = fake_core(|request| {
        let mut shell = agent_terminal("idle");
        shell["address"] = json!("t4");
        shell["status"] = Value::Null;
        vec![result(
            request,
            json!({"hidden_finished": 0, "machines": [], "terminals": [agent_terminal("idle"), shell]}),
        )]
    });

    let agents = client.list_agents().unwrap();

    let panes: Vec<&PaneId> = agents.iter().map(|agent| &agent.pane_id).collect();
    assert_eq!(panes, [&pane()]);
}

#[test]
fn an_agent_that_exits_is_told_once_to_the_reviewer() {
    let (client, _core) = fake_core(|request| {
        vec![
            result(
                request,
                json!({"seq": 1, "terminals": [agent_terminal("done")]}),
            ),
            notification("heartbeat", json!({})),
            // herdr-app's layout changed: a document, of no terminal.
            notification(
                "event",
                json!({"type": "doc", "seq": 2, "data": {"name": "herdr-app", "version": 1}}),
            ),
            event(
                "status",
                json!({"records": {}, "reported": true, "status": null}),
            ),
            event("finished", json!({"exit_code": 0, "reason": "exited"})),
            // Another agent arrives after: the end of the first one's terminal told nothing.
            event_of("t4", "created", {
                let mut terminal = agent_terminal("idle");
                terminal["address"] = json!("t4");
                terminal
            }),
        ]
    });
    let mut told = Vec::new();

    client
        .forward_events(&EventCanceller::default(), &mut |event| {
            told.push(event);
            told.len() < 2
        })
        .unwrap();

    assert_eq!(
        told,
        [
            HerdrEvent::AgentDetected {
                pane_id: pane(),
                workspace_id: WorkspaceId("sp1".into()),
                agent: Some("claude".into()),
                released: true,
                final_status: Some(AgentStatus::Done),
            },
            HerdrEvent::AgentDetected {
                pane_id: PaneId("t4".into()),
                workspace_id: WorkspaceId("sp1".into()),
                agent: Some("claude".into()),
                released: false,
                final_status: None,
            },
        ]
    );
}

#[test]
fn a_subscriber_that_fell_behind_loses_its_stream() {
    let (client, _core) = fake_core(|request| {
        vec![
            result(request, json!({"seq": 1, "terminals": []})),
            notification("missed", json!({"seq": 9})),
        ]
    });

    assert!(matches!(
        client.forward_events(&EventCanceller::default(), &mut |_| true),
        Err(Error::Protocol { .. })
    ));
}

#[test]
fn an_agents_statuses_start_with_the_one_it_has_and_end_with_its_program() {
    let (client, core) = fake_core(|request| {
        vec![
            result(
                request,
                json!({"seq": 1, "terminals": [agent_terminal("working")]}),
            ),
            event(
                "status",
                json!({"records": {}, "reported": true, "status": {"app": "claude-code", "state": "idle"}}),
            ),
            event("turn", json!({"state": "idle", "turn": 1})),
            event(
                "status",
                json!({"records": {}, "reported": true, "status": {"app": "claude-code", "state": "blocked"}}),
            ),
            event("finished", json!({"exit_code": 0, "reason": "exited"})),
        ]
    });
    let mut statuses = Vec::new();

    client
        .forward_agent_status(&pane(), &EventCanceller::default(), &mut |status| {
            statuses.push(status);
            true
        })
        .unwrap();

    assert_eq!(
        statuses,
        [
            AgentStatus::Working,
            AgentStatus::Idle,
            AgentStatus::Blocked
        ]
    );
    assert_eq!(
        core.requests.recv().unwrap()["params"],
        json!({"terminal": "t3"})
    );
}

#[test]
fn a_cancelled_stream_ends_without_an_error() {
    let (client, _core) = fake_core(|request| {
        vec![
            result(
                request,
                json!({"seq": 1, "terminals": [agent_terminal("done")]}),
            ),
            event("finished", json!({"exit_code": 0, "reason": "exited"})),
        ]
    });
    let canceller = EventCanceller::default();
    let stop = canceller.clone();

    // The stream goes on reading once the canceller ended it, as when the pane closes.
    client
        .forward_events(&canceller, &mut |_| {
            stop.cancel();
            true
        })
        .unwrap();
}

#[test]
fn the_statuses_of_a_terminal_herdr_core_no_longer_knows_end_at_once() {
    let (client, _core) = fake_core(|request| vec![refusal_of(request, "not_found")]);
    let mut statuses = Vec::new();

    client
        .forward_agent_status(&pane(), &EventCanceller::default(), &mut |status| {
            statuses.push(status);
            true
        })
        .unwrap();

    assert_eq!(statuses, []);
}

#[test]
fn the_statuses_of_an_agent_whose_program_ended_already_end_at_once() {
    let (client, _core) = fake_core(|request| {
        let mut finished = agent_terminal("idle");
        finished["state"] = json!("finished");
        finished["status"] = Value::Null;
        vec![result(request, json!({"seq": 1, "terminals": [finished]}))]
    });

    client
        .forward_agent_status(&pane(), &EventCanceller::default(), &mut |_| true)
        .unwrap();
}
