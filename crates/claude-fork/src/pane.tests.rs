use super::*;
use agent_fork::ForkCommand;

#[test]
fn a_fork_inherits_no_herdr_variable() {
    let environ = b"HOME=/home/a\0HERDR_PANE_ID=w1:p1\0HERDR_SOCKET_PATH=/s\0PATH=/bin\0";

    let names: Vec<_> = environment(environ)
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    assert_eq!(names, ["HOME", "PATH"]);
}

#[test]
fn the_transcripts_are_under_the_configuration_directory_or_the_home_directory() {
    let mut pane = PaneClaude {
        command: ForkCommand {
            program: "claude".into(),
            arguments: Vec::new(),
            directory: "/work".into(),
            environment: vec![("HOME".into(), "/home/a".into())],
        },
        model: None,
    };
    assert_eq!(
        pane.transcripts().root(),
        std::path::Path::new("/home/a/.claude/projects")
    );

    pane.command
        .environment
        .push(("CLAUDE_CONFIG_DIR".into(), "/config".into()));

    assert_eq!(
        pane.transcripts().root(),
        std::path::Path::new("/config/projects")
    );
}

#[test]
fn an_agent_run_as_a_script_keeps_its_script_and_loses_its_prompt() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("claude");
    std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
    let mut child = std::process::Command::new("sh")
        .arg(&script)
        .args(["--model", "opus", "a prompt"])
        .current_dir(directory.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    // Right after the spawn, the child's command line can still be empty, or this test's own.
    let read = loop {
        let read = PaneClaude::read(child.id());
        let started = read
            .as_ref()
            .is_ok_and(|pane| pane.command.arguments == [script.as_os_str()]);
        if started || std::time::Instant::now() > deadline {
            break read;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    child.kill().unwrap();
    child.wait().unwrap();
    let pane = read.unwrap();

    assert_eq!(pane.command.arguments, [script.as_os_str()]);
    assert_eq!(pane.model.as_deref(), Some("opus"));
    assert_eq!(
        pane.command.directory,
        directory.path().canonicalize().unwrap()
    );
}
