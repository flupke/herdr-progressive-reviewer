//! The Claude Code process of a Herdr pane, as a fork copies it.

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use agent_fork::ForkCommand;

use crate::arguments;
use crate::transcripts::Transcripts;

/// The pane agent's process: what a fork starts with so that it reads the same prompt cache.
#[derive(Clone, Debug)]
pub(crate) struct PaneClaude {
    /// The binary the agent runs, resolved, so a fork runs the same version; its arguments as
    /// a fork keeps them; its directory; its environment without Herdr's variables, so that
    /// Herdr never takes a fork for the pane's agent.
    pub(crate) command: ForkCommand,
    /// The model its arguments name, if any.
    pub(crate) model: Option<String>,
}

impl PaneClaude {
    /// Reads the running process `pid`.
    pub(crate) fn read(pid: u32) -> Result<Self, String> {
        let proc = PathBuf::from(format!("/proc/{pid}"));
        let read = |what: &str| format!("cannot read the agent's {what}");
        let program = fs::read_link(proc.join("exe")).map_err(|_| read("program"))?;
        let directory = fs::read_link(proc.join("cwd")).map_err(|_| read("directory"))?;
        let command = fs::read(proc.join("cmdline")).map_err(|_| read("command line"))?;
        let environ = fs::read(proc.join("environ")).map_err(|_| read("environment"))?;
        let command: Vec<String> = command
            .split(|byte| *byte == 0)
            .filter(|argument| !argument.is_empty())
            .map(|argument| String::from_utf8_lossy(argument).into_owned())
            .collect();
        // An agent started as a script, through its interpreter, names the script first.
        let script = command
            .get(1)
            .filter(|first| !first.starts_with('-') && directory.join(first).is_file());
        let rest = command
            .get(1 + usize::from(script.is_some())..)
            .unwrap_or_default();
        let (kept, model) = arguments::kept(rest)?;
        Ok(Self {
            command: ForkCommand {
                program,
                arguments: script
                    .cloned()
                    .into_iter()
                    .chain(kept)
                    .map(OsString::from)
                    .collect(),
                directory,
                environment: environment(&environ),
            },
            model,
        })
    }

    /// Where the agent keeps its transcripts: `projects` under its configuration directory.
    pub(crate) fn transcripts(&self) -> Transcripts {
        let variable = |name: &str| {
            self.command
                .environment
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| PathBuf::from(value))
        };
        let configuration = variable("CLAUDE_CONFIG_DIR")
            .or_else(|| variable("HOME").map(|home| home.join(".claude")))
            .unwrap_or_else(|| PathBuf::from("/nonexistent/.claude"));
        Transcripts::at(configuration.join("projects"))
    }
}

/// The variables of a process environment, as `/proc/<pid>/environ` holds them, except
/// Herdr's.
fn environment(environ: &[u8]) -> Vec<(OsString, OsString)> {
    environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let at = entry.iter().position(|byte| *byte == b'=')?;
            let (name, value) = entry.split_at(at);
            (!name.starts_with(b"HERDR_")).then(|| {
                (
                    OsString::from_vec(name.to_vec()),
                    OsString::from_vec(value[1..].to_vec()),
                )
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "pane.tests.rs"]
mod tests;
