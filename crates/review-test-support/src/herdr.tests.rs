use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;

use herdr_client::protocol::HerdrEvent;

use super::*;

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

    // The server's shell stops it, with SIGKILL at the latest, then exits.
    child.wait().unwrap();
    assert!(UnixStream::connect(&server.socket_path).is_err());
    assert!(
        !server.root().exists(),
        "the private directory of a server whose owner died is removed"
    );
}

#[test]
fn agents_are_detected_with_the_rules_the_repository_keeps() {
    let repository = tempfile::tempdir().unwrap();
    let server = HerdrTestServer::start(repository.path());
    for rules in &DetectionRules::ALL {
        let pane = server.pane(rules.agent());
        let events = server.events();
        server.run_fake_agent(&pane, rules.agent(), "✳ Ready");

        events.wait_for("the agent's detection", |event| {
            matches!(event, HerdrEvent::AgentDetected { pane_id, agent, .. }
                if *pane_id == pane && agent.as_deref() == Some(rules.agent()))
        });

        let explanation = server.explain_agent(&pane);
        assert_eq!(explanation["state"], "idle", "{explanation}");

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
    /// A pane in a new workspace labelled `label`.
    fn pane(&self, label: &str) -> PaneId {
        let workspace = self.run_cli_json(&[
            "workspace",
            "create",
            "--cwd",
            &self.root().to_string_lossy(),
            "--label",
            label,
            "--no-focus",
        ]);
        PaneId(
            workspace["result"]["root_pane"]["pane_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
    }

    /// Start a process named `agent` in `pane` that shows `title` and its prompt.
    fn run_fake_agent(&self, pane: &PaneId, agent: &str, title: &str) {
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
        self.run_cli(&["pane", "run", &pane.0, &script.to_string_lossy()]);
    }

    /// Herdr's explanation of the agent it detects in `pane`.
    fn explain_agent(&self, pane: &PaneId) -> serde_json::Value {
        let output = self
            .command()
            .args(["agent", "explain", &pane.0, "--json"])
            .output()
            .unwrap();
        let mut explanation: serde_json::Value =
            serde_json::from_slice(&output.stdout).unwrap_or_default();
        // Keep failure messages short: every rule's evidence is long.
        if let Some(fields) = explanation.as_object_mut() {
            fields.remove("evaluated_rules");
        }
        explanation
    }
}
