//! A fork of the stand-in: it records its arguments, environment and prompt in its directory,
//! copies its parent's transcript, then works until it is told to end its turn or the reviewer
//! stops it. Once told to submit its turn, its output says that its submit has its answer. It
//! reports each step on the event socket, and takes its commands there.

use std::fs;
use std::io::{self, Read as _, Write as _};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use review_test_support::stand_in::{StandInCommand, StandInConnection, StandInEvent, StandInRole};
use stand_in_agent::{StandInDirectory, variables};

use crate::transcript::own_configuration;

/// Runs the fork that Claude Code's `arguments` start.
pub(crate) fn run(arguments: &[String]) {
    let value = |name: &str| {
        let at = arguments
            .iter()
            .position(|argument| argument == name)
            .unwrap_or_else(|| panic!("a fork takes {name}"));
        arguments[at + 1].clone()
    };
    let directory = StandInDirectory::new(PathBuf::from(
        std::env::var_os(variables::DIRECTORY).expect("the stand-in's directory"),
    ));
    if let Ok(real) = fs::read_to_string(directory.real_claude()) {
        // The real Claude Code takes the fork's turn, with the pane's flags it names.
        let mut real = real.lines();
        let program = real.next().unwrap();
        let from = arguments
            .iter()
            .position(|argument| argument == "-p")
            .unwrap();
        let error = Command::new(program)
            .args(real)
            .args(&arguments[from..])
            .exec();
        panic!("could not run {program}: {error}");
    }
    let (session, parent) = (value("--session-id"), value("--resume"));
    let (report, commands) = StandInConnection::from_env(StandInRole::Fork {
        session: session.clone(),
    })
    .expect("the event socket");
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt).unwrap();
    let configuration = own_configuration().expect("Claude Code's configuration directory");
    let mut history =
        fs::read_to_string(stand_in_agent::transcript(&configuration, &parent)).unwrap();
    history.push_str("{\"type\":\"user\",\"uuid\":\"fork-prompt\"}\n");
    fs::write(
        stand_in_agent::transcript(&configuration, &session),
        history,
    )
    .unwrap();
    let names: Vec<_> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    let record = |kind: &str, content: &str| {
        let path = directory.fork_record(&session, kind);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    };
    record("args", &arguments.join("\n"));
    record("env", &names.join("\n"));
    // The fork reads the turn's files as it reads the prompt that names them.
    let prompt = review_test_support::turn::with_turn(&prompt);
    record("prompt", &prompt);
    report.report(&StandInEvent::ForkStarted { parent, prompt });
    let usage = r#"{"input_tokens":3,"cache_creation_input_tokens":20,"cache_read_input_tokens":1000,"output_tokens":7}"#;
    println!(r#"{{"type":"system","subtype":"init","session_id":"{session}"}}"#);
    println!(r#"{{"type":"assistant","message":{{"id":"m1","usage":{usage}}}}}"#);
    io::stdout().flush().unwrap();
    let mut answered = false;
    // A closed socket means that nobody follows the fork any more.
    for command in commands {
        match command {
            StandInCommand::End => break,
            StandInCommand::Submit if !answered => {
                answered = true;
                println!(
                    r#"{{"type":"assistant","message":{{"id":"m2","content":[{{"type":"tool_use","id":"submit","name":"mcp__herdr_reviewer__submit_question","input":{{}}}}],"usage":{usage}}}}}"#
                );
                println!(
                    r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"submit"}}]}}}}"#
                );
                io::stdout().flush().unwrap();
                report.report(&StandInEvent::ForkSubmitted);
            }
            StandInCommand::Submit => {}
            command => panic!("a fork got the agent's command {command:?}"),
        }
    }
    println!(r#"{{"type":"result","subtype":"success","is_error":false}}"#);
    io::stdout().flush().unwrap();
    report.report(&StandInEvent::ForkFinished);
}
