//! Stopping a fork: SIGTERM, then SIGKILL when it still runs three seconds later.

use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::stamp::ProcessStamp;

/// How long a fork has to end after SIGTERM before it gets SIGKILL.
pub(crate) const TERM_WAIT: Duration = Duration::from_secs(3);
/// How long the stop waits for a fork to end after SIGKILL.
pub(crate) const KILL_WAIT: Duration = Duration::from_secs(2);
/// How often a stop looks whether a process it did not start has ended.
const RECORDED_POLL: Duration = Duration::from_millis(50);

/// Sends `signal` to the process `stamp` names, unless that process no longer runs.
pub(crate) fn signal(stamp: ProcessStamp, signal: Signal) {
    if !stamp.is_running() {
        return;
    }
    if let Ok(pid) = i32::try_from(stamp.pid) {
        let _ = kill(Pid::from_raw(pid), signal);
    }
}

/// Stops a fork that an earlier reviewer started and recorded as `stamp`, when that process
/// still runs and its command line holds `needle` (the fork's session ID): SIGTERM, then
/// SIGKILL three seconds later. Waits until it ended, at most five seconds; whether it ran.
pub fn stop_recorded(stamp: ProcessStamp, needle: &str) -> bool {
    if !stamp.runs_with(needle) {
        return false;
    }
    signal(stamp, Signal::SIGTERM);
    if !ended_within(stamp, TERM_WAIT) {
        signal(stamp, Signal::SIGKILL);
        ended_within(stamp, KILL_WAIT);
    }
    true
}

/// Whether the process `stamp` names ends within `limit`. Its parent reaps it, not this
/// process, so the stop looks again every 50 ms.
fn ended_within(stamp: ProcessStamp, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while stamp.is_running() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(RECORDED_POLL);
    }
    true
}
