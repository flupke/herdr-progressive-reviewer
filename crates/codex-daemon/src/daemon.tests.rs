use std::ffi::OsString;
use std::path::{Path, PathBuf};

use herdr_client::protocol::PaneProcess;

use super::*;

fn socket_of(arguments: &[&str], variables: &[(&str, &str)]) -> Result<PathBuf, NoDaemon> {
    let arguments: Vec<String> = arguments.iter().map(|&argument| argument.into()).collect();
    Daemon::of_command(&arguments, Path::new("/work/repo"), |name| {
        variables
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| OsString::from(value))
    })
    .map(|daemon| daemon.socket)
}

#[test]
fn the_daemon_listens_in_the_codex_home_or_under_home() {
    assert_eq!(
        socket_of(&[], &[("CODEX_HOME", "/homes/codex"), ("HOME", "/home/me")]),
        Ok("/homes/codex/app-server-control/app-server-control.sock".into())
    );
    assert_eq!(
        socket_of(&["resume", "t1"], &[("HOME", "/home/me")]),
        Ok("/home/me/.codex/app-server-control/app-server-control.sock".into())
    );
}

#[test]
fn a_remote_unix_socket_is_the_daemon_relative_to_the_agent_s_directory() {
    let home = [("CODEX_HOME", "/homes/codex")];

    assert_eq!(
        socket_of(&["--remote", "unix:///run/codex.sock"], &home),
        Ok("/run/codex.sock".into())
    );
    assert_eq!(
        socket_of(&["--remote=unix://codex.sock"], &home),
        Ok("/work/repo/codex.sock".into())
    );
    assert_eq!(
        socket_of(&["--remote", "unix://"], &home),
        Ok("/homes/codex/app-server-control/app-server-control.sock".into())
    );
}

#[test]
fn an_agent_on_another_server_or_none_has_no_daemon() {
    let home = [("CODEX_HOME", "/homes/codex")];

    assert_eq!(
        socket_of(&["--remote", "ws://127.0.0.1:4500"], &home),
        Err(NoDaemon::Remote("ws://127.0.0.1:4500".to_owned()))
    );
    assert_eq!(
        socket_of(&["--remote", "unix://", "--no-daemon"], &home),
        Err(NoDaemon::WithoutDaemon)
    );
}

#[test]
fn a_flag_after_the_end_of_options_is_a_prompt() {
    assert_eq!(
        socket_of(&["--", "--no-daemon"], &[("CODEX_HOME", "/homes/codex")]),
        Ok("/homes/codex/app-server-control/app-server-control.sock".into())
    );
}

#[test]
fn a_pane_without_codex_has_no_daemon() {
    let shell = PaneProcess {
        pid: std::process::id(),
        name: "zsh".to_owned(),
        argv: Some(vec!["-zsh".to_owned()]),
    };

    assert_eq!(Daemon::of_pane(&[shell]), Err(NoDaemon::NoCodex));
}

#[test]
fn a_node_wrapper_s_arguments_are_codex_s_alone() {
    // The npm package's wrapper, as Herdr lists it when the native binary is not listed. A
    // Codex option before the script is Node's, not Codex's.
    let wrapper = |arguments: &[&str]| PaneProcess {
        pid: std::process::id(),
        name: "node".to_owned(),
        argv: Some(
            [
                "node",
                "--no-daemon",
                "/usr/lib/node_modules/@openai/codex/bin/codex",
            ]
            .iter()
            .chain(arguments)
            .map(|&argument| argument.to_owned())
            .collect(),
        ),
    };

    assert!(Daemon::of_pane(&[wrapper(&[])]).is_ok());
    assert_eq!(
        Daemon::of_pane(&[wrapper(&["--no-daemon"])]),
        Err(NoDaemon::WithoutDaemon)
    );
}
