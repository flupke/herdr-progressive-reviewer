//! Claude Code's transcripts: `projects/<directory>/<session>.jsonl` under its configuration
//! directory, one line per entry.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// The transcripts of one Claude Code configuration.
#[derive(Clone, Debug)]
pub struct Transcripts {
    projects: PathBuf,
}

/// The last conversation entry of a session: a later one means the session moved.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LastEntry {
    /// The `uuid` of its last `user` or `assistant` entry.
    pub(crate) entry: Option<String>,
    /// The model of its last assistant entry.
    pub(crate) model: Option<String>,
}

impl Transcripts {
    /// The transcripts under the projects directory `projects`.
    pub fn at(projects: PathBuf) -> Self {
        Self { projects }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.projects
    }

    /// The transcript of `session`, in whichever project directory holds it.
    pub fn find(&self, session: &str) -> Option<PathBuf> {
        let name = format!("{session}.jsonl");
        fs::read_dir(&self.projects)
            .ok()?
            .flatten()
            .map(|entry| entry.path().join(&name))
            .find(|path| path.is_file())
    }

    /// The last conversation entry of `session`; `None` when it has no transcript. Lines that
    /// are not conversation, such as the metadata an agent appends when it exits or resumes,
    /// do not count.
    pub(crate) fn last_entry(&self, session: &str) -> Option<LastEntry> {
        let file = fs::File::open(self.find(session)?).ok()?;
        let mut last = LastEntry::default();
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            // Most lines are tool results: skip them before parsing.
            if !line.contains("\"type\":\"user\"") && !line.contains("\"type\":\"assistant\"") {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };
            let kind = value.get("type").and_then(serde_json::Value::as_str);
            if !matches!(kind, Some("user" | "assistant")) {
                continue;
            }
            last.entry = value
                .get("uuid")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            if let Some(model) = value
                .pointer("/message/model")
                .and_then(serde_json::Value::as_str)
                .filter(|model| kind == Some("assistant") && *model != "<synthetic>")
            {
                last.model = Some(model.to_owned());
            }
        }
        Some(last)
    }

    /// Deletes the transcript of `session` and the directory of the same name beside it, which
    /// holds its tool results and subagents. Only sessions run-ahead created are deleted.
    pub(crate) fn delete(&self, session: &str) {
        let Some(path) = self.find(session) else {
            return;
        };
        let _ = fs::remove_file(&path);
        let side = path.with_extension("");
        if side.is_dir() {
            let _ = fs::remove_dir_all(side);
        }
    }
}

#[cfg(test)]
#[path = "transcripts.tests.rs"]
mod tests;
