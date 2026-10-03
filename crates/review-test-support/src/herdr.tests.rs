use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::eventually;

#[test]
fn the_server_stops_when_its_owner_is_killed() {
    let repository = tempfile::tempdir().unwrap();
    let mut server = HerdrTestServer::start(repository.path());
    let child = server.child.as_mut().unwrap();
    // Hand the only writing end of the server's pipe to a stand-in owner.
    let mut owner = Command::new("sleep")
        .arg("60")
        .stdout(Stdio::from(child.stdin.take().unwrap()))
        .spawn()
        .unwrap();

    owner.kill().unwrap();
    owner.wait().unwrap();

    assert!(eventually(Duration::from_secs(5), || child
        .try_wait()
        .unwrap()
        .is_some()));
    assert!(UnixStream::connect(&server.socket_path).is_err());
}

#[test]
fn agents_are_detected_with_the_rules_the_repository_keeps() {
    let repository = tempfile::tempdir().unwrap();
    let server = HerdrTestServer::start(repository.path());
    for rules in &DetectionRules::ALL {
        let pane = server.run_fake_agent(rules.agent(), "✳ Ready");

        let explanation = server.wait_for_agent_state(&pane, "idle");

        let expected = rules.path(&herdr_config_directory(server.root()));
        assert_eq!(
            explanation["manifest_source"].as_str(),
            expected.to_str(),
            "Herdr did not use the repository's {} rules: {explanation}",
            rules.agent()
        );
    }
}

#[test]
fn a_dropped_server_stops_answering() {
    let repository = tempfile::tempdir().unwrap();
    let server = HerdrTestServer::start(repository.path());
    let socket_path = server.socket_path.clone();

    drop(server);

    assert!(UnixStream::connect(socket_path).is_err());
}

impl HerdrTestServer {
    /// Start a process named `agent` that shows `title` and its prompt.
    fn run_fake_agent(&self, agent: &str, title: &str) -> String {
        let script = self.root().join(agent);
        fs::write(
            &script,
            // The shell keeps running, so the foreground process keeps the
            // agent's name.
            format!(
                "#!/bin/sh\nprintf '\\033]0;{title}\\007\\033[2J\\033[H> '\n\
                 while :; do read -r line || sleep 1; done\n"
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let workspace = self.run_cli_json(&[
            "workspace",
            "create",
            "--cwd",
            &self.root().to_string_lossy(),
            "--label",
            agent,
            "--no-focus",
        ]);
        let pane = workspace["result"]["root_pane"]["pane_id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.run_cli(&["pane", "run", &pane, &script.to_string_lossy()]);
        pane
    }

    /// Wait until Herdr's detection puts the agent in `pane` in `state`, and
    /// return Herdr's explanation of it.
    fn wait_for_agent_state(&self, pane: &str, state: &str) -> serde_json::Value {
        let mut explanation = serde_json::Value::Null;
        let reached = eventually(Duration::from_secs(30), || {
            // Herdr answers `agent_not_found` until it detects the agent.
            let output = self
                .command()
                .args(["agent", "explain", pane, "--json"])
                .output()
                .unwrap();
            explanation = serde_json::from_slice(&output.stdout).unwrap_or_default();
            explanation["state"] == state
        });
        // Keep failure messages short: every rule's evidence is long.
        if let Some(fields) = explanation.as_object_mut() {
            fields.remove("evaluated_rules");
        }
        assert!(reached, "Herdr did not detect {state}: {explanation}");
        explanation
    }
}
