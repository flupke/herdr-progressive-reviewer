//! The hook that a real Claude Code runs to report what it does as the stand-in agent reports it.

use std::io::Write;
use std::process::{Command, Stdio};

use review_test_support::stand_in::{EVENTS_VARIABLE, StandInEvent, StandInEvents, StandInRole};

/// What the hook that `hooks` names for Claude Code's event `event` reports, run as Claude Code
/// runs a command hook: through the shell, with the event on its standard input, here under a
/// shell whose command line is `shell_arguments`.
fn run_hook(
    hooks: &serde_json::Value,
    events: &StandInEvents,
    socket: &std::path::Path,
    event: &serde_json::Value,
    shell_arguments: &[&str],
) -> Vec<StandInEvent> {
    let name = event["hook_event_name"].as_str().unwrap();
    let command = hooks[name][0]["hooks"][0]["command"].as_str().unwrap();
    let mut hook = Command::new("sh")
        // `; :` keeps the shell from replacing itself with the hook, so that the shell, whose
        // command line is `shell_arguments`, stays the hook's parent, as Claude Code is.
        .args(["-c", &format!("{command}; :")])
        .args(shell_arguments)
        .env(EVENTS_VARIABLE, socket)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    assert!(hook.wait().unwrap().success());
    events
        .events_until("the hook's end", |reported| {
            reported.event == StandInEvent::Disconnected
        })
        .into_iter()
        .inspect(|reported| assert_eq!(reported.from, StandInRole::Agent))
        .map(|reported| reported.event)
        .filter(|event| {
            !matches!(
                event,
                StandInEvent::Hello { .. } | StandInEvent::Disconnected
            )
        })
        .collect()
}

#[test]
fn a_real_agent_s_hooks_report_its_prompts_and_resumes_as_the_stand_in_does() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("events.sock");
    let events = StandInEvents::listen(&socket);
    let hooks =
        stand_in_agent::observing_hooks(std::path::Path::new(env!("CARGO_BIN_EXE_stand-in-agent")));
    let observe = |event: serde_json::Value| run_hook(&hooks, &events, &socket, &event, &[]);

    assert_eq!(
        observe(
            serde_json::json!({"hook_event_name": "SessionStart", "session_id": "own",
            "source": "startup"})
        ),
        [StandInEvent::SessionReported {
            session: "own".into(),
            start: "startup".into()
        }]
    );
    // Claude Code wraps a long paste, such as a prompt Herdr types.
    assert_eq!(
        observe(
            serde_json::json!({"hook_event_name": "UserPromptSubmit", "session_id": "own",
            "prompt": "<pasted_content id=\"7\">\nExplore request: r1\n</pasted_content id=\"7\">\n"})
        ),
        [StandInEvent::PromptReceived {
            text: "Explore request: r1\n".into()
        }]
    );
    assert_eq!(
        observe(
            serde_json::json!({"hook_event_name": "SessionStart", "session_id": "fork",
            "source": "resume"})
        ),
        [
            StandInEvent::ResumeReceived {
                session: "fork".into()
            },
            StandInEvent::SessionReported {
                session: "fork".into(),
                start: "resume".into()
            },
        ]
    );
    assert_eq!(
        observe(
            serde_json::json!({"hook_event_name": "SessionStart", "session_id": "helper",
            "source": "startup", "agent_id": "subagent"})
        ),
        [],
        "a subagent's session is not the agent's"
    );
    // A fork runs the agent's hooks too, under the fork's command line.
    let fork_command_line = ["claude", "-p", "--resume", "own", "--fork-session"];
    for event in [
        serde_json::json!({"hook_event_name": "SessionStart", "session_id": "fork",
            "source": "resume"}),
        serde_json::json!({"hook_event_name": "UserPromptSubmit", "session_id": "fork",
            "prompt": "Explore request: r1\n"}),
    ] {
        assert_eq!(
            run_hook(&hooks, &events, &socket, &event, &fork_command_line),
            [],
            "a fork's session is not the agent's"
        );
    }
}
