//! The hooks Claude Code runs for the stand-in agent.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use review_test_support::stand_in::{StandInConnection, StandInEvent};

/// The hooks Claude Code runs for the agent. Herdr's session hook reports its session to Herdr:
/// from Claude Code's `startup` or `resume`, each report newer than the last. With the
/// reviewer's plugin, run by `control`, its hooks run first, as Claude Code runs them: Herdr may
/// hear of a session after the reviewer.
pub(crate) struct Hooks {
    /// The `herdr` the agent reports with.
    pub(crate) herdr: PathBuf,
    pub(crate) pane: String,
    pub(crate) agent: String,
    /// The `reviewer-control` that runs the plugin's hooks, if the agent runs them.
    pub(crate) control: Option<PathBuf>,
    /// The session the agent runs.
    pub(crate) session: String,
    pub(crate) report: Arc<StandInConnection>,
}

impl Hooks {
    /// Whether the plugin's hook blocks the prompt `prompt`, which a command of Claude Code's
    /// own is not.
    pub(crate) fn blocks(&self, prompt: &str) -> bool {
        let Some(control) = &self.control else {
            return false;
        };
        let decision = run_plugin_hook(
            control,
            &serde_json::json!({
                "session_id": self.session,
                "transcript_path": "",
                "cwd": std::env::current_dir().unwrap(),
                "permission_mode": "default",
                "hook_event_name": "UserPromptSubmit",
                "prompt": prompt,
            }),
        );
        !decision.is_empty()
            && serde_json::from_str::<serde_json::Value>(&decision).unwrap()["decision"] == "block"
    }

    /// The agent started the session `session`, from `start`.
    pub(crate) fn session_started(&mut self, session: &str, start: &str) {
        session.clone_into(&mut self.session);
        if let Some(control) = &self.control {
            run_plugin_hook(
                control,
                &serde_json::json!({
                    "session_id": session,
                    "transcript_path": "",
                    "cwd": std::env::current_dir().unwrap(),
                    "hook_event_name": "SessionStart",
                    "source": start,
                }),
            );
        }
        let sequence = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string();
        let status = Command::new(&self.herdr)
            .args([
                "pane",
                "report-agent-session",
                &self.pane,
                "--source",
                &format!("herdr:{}", self.agent),
                "--agent",
                &self.agent,
                "--seq",
                &sequence,
                "--agent-session-id",
                session,
                "--session-start-source",
                start,
            ])
            .status()
            .unwrap();
        assert!(status.success());
        self.report.report(&StandInEvent::SessionReported {
            session: session.to_owned(),
            start: start.to_owned(),
        });
    }
}

/// Runs the hook of the reviewer's Claude Code plugin through `control` on Claude Code's event
/// `event`, as Claude Code does, and returns what it printed.
fn run_plugin_hook(control: &PathBuf, event: &serde_json::Value) -> String {
    let mut hook = Command::new(control)
        .arg(claude_hooks::SUBCOMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = hook.wait_with_output().unwrap();
    assert!(output.status.success(), "the plugin's hook failed");
    String::from_utf8(output.stdout).unwrap()
}
