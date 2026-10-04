//! Agent processes that the reviewer forks and that must not outlive it.
//!
//! A fork is a plain child of the reviewer: it stays in the reviewer's process group, so the
//! hang-up of the reviewer's terminal reaches it when the reviewer dies in its pane. It also
//! gets a parent-death signal: a small wrapper arms it, then runs the agent in its place. The
//! signal fires when the thread that started the wrapper ends, so every fork starts from the
//! [`Launcher`]'s own thread, which lives as long as the launcher. The reviewer stops a fork
//! itself with SIGTERM, then SIGKILL after three seconds. A [`ProcessStamp`] names a process
//! across a restart of the reviewer: its ID and its start time, so a reused ID is never taken
//! for the fork.

mod launcher;
mod output;
mod stamp;
mod stop;
mod wrapper;

pub use launcher::{ForkCommand, Launcher, RunningFork};
pub use output::{Exit, ForkOutput};
pub use stamp::ProcessStamp;
pub use stop::stop_recorded;
pub use wrapper::{Wrapper, run_wrapper};
