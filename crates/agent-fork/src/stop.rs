//! Stopping a fork: SIGTERM, then SIGKILL when it still runs three seconds later.

use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

use crate::stamp::ProcessStamp;

/// How long a stop waits for a fork to end after each signal.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StopWaits {
    /// How long a fork has to end after SIGTERM before it gets SIGKILL.
    pub(crate) term: Duration,
    /// How long the stop waits for a fork to end after SIGKILL.
    pub(crate) kill: Duration,
}

impl Default for StopWaits {
    fn default() -> Self {
        Self {
            term: Duration::from_secs(3),
            kill: Duration::from_secs(2),
        }
    }
}

/// How often a stop looks whether a process it did not start has ended, on a system that
/// cannot tell it when that process ends.
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
    let waits = StopWaits::default();
    signal(stamp, Signal::SIGTERM);
    if !ended_within(stamp, waits.term) {
        signal(stamp, Signal::SIGKILL);
        ended_within(stamp, waits.kill);
    }
    true
}

/// Whether the process `stamp` names ends within `limit`. Its parent reaps it, not this
/// process, so the stop waits on a process descriptor, which the kernel marks readable once
/// the process ended; where there is none, it looks again every 50 ms.
fn ended_within(stamp: ProcessStamp, limit: Duration) -> bool {
    #[cfg(target_os = "linux")]
    if let Some(ended) = descriptor::ended_within(stamp, limit) {
        return ended;
    }
    let deadline = Instant::now() + limit;
    while stamp.is_running() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(RECORDED_POLL);
    }
    true
}

/// Waiting on a process descriptor (`pidfd_open`, Linux 5.3) for the end of a process that
/// this one did not start.
#[cfg(target_os = "linux")]
mod descriptor {
    use std::time::{Duration, Instant};

    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use rustix::process::{Pid, PidfdFlags, pidfd_open};

    use crate::stamp::ProcessStamp;

    /// Whether the process `stamp` names ends within `limit`; `None` when the system gives no
    /// descriptor for it.
    pub(super) fn ended_within(stamp: ProcessStamp, limit: Duration) -> Option<bool> {
        let pid = Pid::from_raw(i32::try_from(stamp.pid).ok()?)?;
        let process = match pidfd_open(pid, PidfdFlags::empty()) {
            Ok(process) => process,
            Err(Errno::SRCH) => return Some(true),
            Err(_) => return None,
        };
        // The ID may name a later process by now: the descriptor names the stamp's process
        // only when that one still runs.
        if !stamp.is_running() {
            return Some(true);
        }
        let deadline = Instant::now() + limit;
        loop {
            let left =
                Timespec::try_from(deadline.saturating_duration_since(Instant::now())).ok()?;
            let mut descriptors = [PollFd::new(&process, PollFlags::IN)];
            match poll(&mut descriptors, Some(&left)) {
                Ok(ready) => return Some(ready > 0),
                Err(Errno::INTR) => {}
                Err(_) => return None,
            }
        }
    }
}
