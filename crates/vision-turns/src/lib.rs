//! Test tooling for `make vision` only: a record of every prompt the reviewer sends its agent,
//! for a vision session's scripted agent. One numbered JSON file per prompt, written once its
//! delivery finished, so the agent reads what was sent instead of parsing a terminal. The
//! Explore session and the review threads' worker write to the same log, each prompt with its
//! `kind`.
//!
//! The files hold the access value of each prompt, so a reviewer outside a vision session never
//! has a turn log.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

/// The directory a vision session reads its turns from. Its clones share one count, so that
/// every writer numbers its turns after the others'.
#[derive(Clone, Debug)]
pub struct TurnLog {
    directory: PathBuf,
    recorded: Arc<AtomicU64>,
}

/// A sent turn with its number and how its delivery ended.
#[derive(Serialize)]
struct RecordedTurn<'a, T> {
    turn: u64,
    delivered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(flatten)]
    sent: &'a T,
}

impl TurnLog {
    /// Continue after the highest turn `directory` already holds, so a reopened reviewer never
    /// reuses a number.
    pub fn open(directory: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&directory)?;
        let recorded = std::fs::read_dir(&directory)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()?
                    .strip_prefix("turn-")?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        Ok(Self {
            directory,
            recorded: Arc::new(AtomicU64::new(recorded)),
        })
    }

    /// Save `sent` under the next number, with the error its delivery ended on, if any. A file
    /// appears complete or not at all.
    pub fn record(&self, sent: &impl Serialize, error: Option<String>) {
        let turn = self.recorded.fetch_add(1, Ordering::AcqRel) + 1;
        let recorded = RecordedTurn {
            turn,
            delivered: error.is_none(),
            error,
            sent,
        };
        let path = self.directory.join(format!("turn-{turn:06}.json"));
        let partial = path.with_extension("partial");
        // A vision session is a test tool: a turn it cannot save shows as a turn the scripted
        // agent never receives.
        if let Ok(bytes) = serde_json::to_vec_pretty(&recorded)
            && std::fs::write(&partial, bytes).is_ok()
        {
            let _ = std::fs::rename(partial, path);
        }
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
