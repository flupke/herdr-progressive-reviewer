//! A process named by its ID and its start time, which together never name another process.

use std::fs;

use serde::{Deserialize, Serialize};

/// A process ID with the process's start time, in clock ticks since the boot, as the kernel
/// reports it in `/proc/<pid>/stat`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProcessStamp {
    pub pid: u32,
    pub started: u64,
}

impl ProcessStamp {
    /// The stamp of the running process `pid`; `None` when no such process runs, or only its
    /// zombie, which waits for its parent to read its exit status.
    pub fn of(pid: u32) -> Option<Self> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (state, started) = state_and_start_time(&stat)?;
        (!matches!(state, "Z" | "X")).then_some(Self { pid, started })
    }

    /// The stamp of the process `pid`, or, when it cannot be read, one with an unknown start
    /// time, which no running process matches.
    pub fn read(pid: u32) -> Self {
        Self::of(pid).unwrap_or(Self { pid, started: 0 })
    }

    /// Whether the process still runs: a process with its ID runs, and started when it did.
    pub fn is_running(&self) -> bool {
        Self::of(self.pid).is_some_and(|now| now.started == self.started)
    }

    /// Whether the process still runs and its command line holds `needle`, such as the
    /// session ID a fork was started with.
    pub fn runs_with(&self, needle: &str) -> bool {
        self.is_running()
            && fs::read(format!("/proc/{}/cmdline", self.pid)).is_ok_and(|command| {
                command
                    .split(|byte| *byte == 0)
                    .any(|argument| argument == needle.as_bytes())
            })
    }
}

/// The state (the 3rd field) and the start time (the 22nd) of a `/proc/<pid>/stat` line,
/// counted after the command name, which is in parentheses and may hold spaces and parentheses
/// itself.
fn state_and_start_time(stat: &str) -> Option<(&str, u64)> {
    let (_, after_name) = stat.rsplit_once(')')?;
    let mut fields = after_name.split_whitespace();
    let state = fields.next()?;
    Some((state, fields.nth(18)?.parse().ok()?))
}

#[cfg(test)]
#[path = "stamp.tests.rs"]
pub(crate) mod tests;
