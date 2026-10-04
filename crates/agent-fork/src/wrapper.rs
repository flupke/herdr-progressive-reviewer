//! The wrapper every fork starts through: it arms the parent-death signal, checks that its
//! parent still runs, then runs the agent in its own place, so the agent keeps the signal and
//! the process ID the reviewer recorded.

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use nix::sys::signal::Signal;

/// The program that wraps each fork, with the arguments that pick its wrapper mode. The
/// launcher adds the reviewer's process ID, then the fork's program and arguments.
#[derive(Clone, Debug)]
pub struct Wrapper {
    pub program: PathBuf,
    pub arguments: Vec<OsString>,
}

/// The wrapper's body: arms SIGTERM as the parent-death signal, exits when the parent `parent`
/// already died, else runs `program` with `arguments` in place of this process. Returns only
/// when the parent died or the program cannot run, with the reason.
fn exec_with_parent_death(
    parent: u32,
    program: &OsString,
    arguments: &[OsString],
) -> std::io::Error {
    // SIGTERM rather than SIGKILL: Claude Code closes its MCP servers itself on SIGTERM.
    if let Err(error) = nix::sys::prctl::set_pdeathsig(Signal::SIGTERM) {
        return error.into();
    }
    // The parent may have died before the signal was armed.
    if u32::try_from(nix::unistd::getppid().as_raw()).ok() != Some(parent) {
        return std::io::Error::other("the reviewer that started this fork is gone");
    }
    Command::new(program).args(arguments).exec()
}

/// The wrapper's command line, after the arguments that pick its mode: the reviewer's process
/// ID, then the fork's program and arguments. Runs the fork; returns only with why it could
/// not.
pub fn run_wrapper(mut arguments: impl Iterator<Item = OsString>) -> std::io::Error {
    let parent = arguments
        .next()
        .and_then(|parent| parent.into_string().ok())
        .and_then(|parent| parent.parse().ok());
    let (Some(parent), Some(program)) = (parent, arguments.next()) else {
        return std::io::Error::other("usage: <reviewer pid> <program> [arguments...]");
    };
    let arguments: Vec<_> = arguments.collect();
    exec_with_parent_death(parent, &program, &arguments)
}
