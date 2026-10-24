//! The transcript the stand-in agent keeps for its session when it stands in for Claude Code.

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use stand_in_agent::variables;

/// Claude Code's configuration directory, from the stand-in's environment.
pub(crate) fn own_configuration() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)
}

/// The transcript of the agent's session, as Claude Code keeps it: a line per prompt it read, with
/// the prompt, and per answer it gave.
pub(crate) struct StandInTranscript {
    /// Claude Code's configuration directory, and the transcript's path in it.
    path: Option<(PathBuf, PathBuf)>,
    turns: usize,
}

impl StandInTranscript {
    /// The transcript of the agent's session; none when the agent keeps no transcript.
    pub(crate) fn open() -> Self {
        let path = std::env::var(variables::SESSION)
            .ok()
            .zip(own_configuration())
            .map(|(session, configuration)| {
                fs::create_dir_all(stand_in_agent::transcripts(&configuration)).unwrap();
                let path = stand_in_agent::transcript(&configuration, &session);
                fs::write(&path, "{\"type\":\"user\",\"uuid\":\"start\"}\n").unwrap();
                (configuration, path)
            });
        Self { path, turns: 0 }
    }

    /// The agent continues the session `session`, whose transcript a fork wrote.
    pub(crate) fn resume(&mut self, session: &str) {
        if let Some((configuration, path)) = &mut self.path {
            *path = stand_in_agent::transcript(configuration, session);
        }
    }

    /// The agent read `prompt` and answered it.
    pub(crate) fn turn(&mut self, prompt: &str) {
        let Some((_, path)) = &self.path else {
            return;
        };
        self.turns += 1;
        let prompt = serde_json::json!({"type": "user", "uuid": format!("prompt-{}", self.turns),
            "message": {"role": "user", "content": prompt}});
        let mut file = File::options().append(true).open(path).unwrap();
        writeln!(
            file,
            "{prompt}\n\
             {{\"type\":\"assistant\",\"uuid\":\"answer-{0}\",\"message\":{{\"model\":\"stand-in-model\"}}}}",
            self.turns
        )
        .unwrap();
    }
}
