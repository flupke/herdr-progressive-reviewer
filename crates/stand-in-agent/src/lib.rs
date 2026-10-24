//! The stand-in agent that the reviewer's tests and `make vision` sessions run in a Herdr pane:
//! a program that plays an agent by name (Claude Code, Codex), and plays a fork of Claude Code's
//! session when run-ahead starts it as one. It reports what it does on the event socket of
//! `review_test_support::stand_in`, and takes its commands there. Run by the hooks of a real
//! Claude Code ([`observing_hooks`]), it reports what that agent does as the stand-in would.
//!
//! This library holds what a test or a vision session that runs the program agrees with it on:
//! where Cargo builds it, the variables it reads, and the files it reads and writes. The program
//! itself is `src/main.rs`.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// The source the stand-in agent reports its state to Herdr under, which a test names to release
/// it.
pub const AGENT_SOURCE: &str = "progressive-reviewer-e2e";

/// The program's name, as Cargo builds it.
const PROGRAM: &str = "stand-in-agent";

/// The program's first argument when a real Claude Code's hook runs it.
pub const OBSERVE: &str = "observe";

/// The variables the stand-in reads, beside Herdr's own and the event socket's
/// (`review_test_support::stand_in::EVENTS_VARIABLE`).
pub mod variables {
    /// Its [`StandInDirectory`](super::StandInDirectory).
    pub const DIRECTORY: &str = "REVIEW_AGENT_E2E_DIRECTORY";
    /// The `herdr` it reports to Herdr with; Herdr's own `HERDR_BIN_PATH` without it.
    pub const HERDR: &str = "REVIEW_AGENT_E2E_HERDR_BIN";
    /// The agent it plays, by Herdr's name: `claude`, `codex`.
    pub const AGENT: &str = "REVIEW_AGENT_E2E_AGENT";
    /// The session it starts on, which it reports to Herdr as Herdr's session hook does; none
    /// when empty or unset.
    pub const SESSION: &str = "REVIEW_AGENT_E2E_AGENT_SESSION";
    /// `0`: Herdr detects the agent and reads its state from its titles. Otherwise the agent
    /// reports its state to Herdr itself, until it is released.
    pub const REPORT_LIFECYCLE: &str = "REVIEW_AGENT_E2E_REPORT_LIFECYCLE";
    /// The `reviewer-control` that runs the hooks of the reviewer's Claude Code plugin, as Claude
    /// Code runs them: without it, the agent runs no hook of the plugin.
    pub const HOOKS: &str = "REVIEW_AGENT_E2E_HOOKS";
    /// `second`: the agent connects as the second agent.
    pub const ROLE: &str = "REVIEW_AGENT_E2E_ROLE";
}

/// The stand-in that Cargo built in the profile directory `profile`, such as `target/debug`.
pub fn program(profile: &Path) -> Result<PathBuf, String> {
    let program = profile.join(PROGRAM);
    if program.is_file() {
        Ok(program)
    } else {
        Err(format!(
            "{} is not built: run `cargo build -p {PROGRAM}`, as `make test` does",
            program.display()
        ))
    }
}

/// The hooks, in Claude Code's settings, with which a real Claude Code has `program` report the
/// prompts it reads and the sessions it starts and resumes, as the stand-in agent reports them,
/// on the event socket its environment names.
pub fn observing_hooks(program: &Path) -> serde_json::Value {
    // Claude Code runs a hook's command through the shell.
    let program = program.to_string_lossy().replace('\'', r"'\''");
    let hook = serde_json::json!([{"hooks": [{
        "type": "command",
        "command": format!("'{program}' {OBSERVE}"),
        "timeout": 10,
    }]}]);
    serde_json::json!({"SessionStart": hook, "UserPromptSubmit": hook})
}

/// Installs `program` as `agent`, a link whose name Herdr detects the agent by, rather than a
/// copy: every test that runs at once installs it.
pub fn install(program: &Path, agent: &Path) -> std::io::Result<()> {
    symlink(program, agent)
}

/// The directory [`variables::DIRECTORY`] names: the switches that must reach the agent before a
/// prompt, which it reads as it reads the prompt, and what its forks record.
#[derive(Clone, Debug)]
pub struct StandInDirectory(PathBuf);

impl StandInDirectory {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// While this file exists, the agent reads each prompt without starting on it.
    pub fn swallow(&self) -> PathBuf {
        self.0.join("swallow")
    }

    /// This file keeps Herdr and the reviewer from hearing of the agent's next `/resume`, as when
    /// Claude Code's session hooks fail; the agent removes it. When it holds a number of
    /// milliseconds, they hear of it that late instead, as when the hooks come late: the agent
    /// reads nothing typed meanwhile.
    pub fn unreported_resume(&self) -> PathBuf {
        self.0.join("unreported-resume")
    }

    /// What the fork of `session` recorded, by `kind`: its arguments (`args`), the names of its
    /// environment's variables (`env`), and the prompt it read with its turn's files (`prompt`).
    pub fn fork_record(&self, session: &str, kind: &str) -> PathBuf {
        self.forks().join(format!("{session}.{kind}"))
    }

    fn forks(&self) -> PathBuf {
        self.0.join("forks")
    }
}

/// The transcript that the stand-in and its forks keep for `session` under Claude Code's
/// configuration directory `configuration` (`CLAUDE_CONFIG_DIR`), as Claude Code does.
pub fn transcript(configuration: &Path, session: &str) -> PathBuf {
    transcripts(configuration).join(format!("{session}.jsonl"))
}

/// The project directory of the stand-in's transcripts under `configuration`.
pub fn transcripts(configuration: &Path) -> PathBuf {
    configuration.join("projects/standin")
}
