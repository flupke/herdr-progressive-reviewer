//! A Cloudflare quick tunnel to a local address: `cloudflared tunnel --url`, which needs no
//! Cloudflare account, and gives a public `https://….trycloudflare.com` address that it prints.
//!
//! [`QuickTunnel::start`] runs `cloudflared` as a fork of the reviewer (`agent-fork`), so the
//! process stays in the reviewer's process group and gets its parent-death signal, and reports
//! once the public address it read, or why there is none: `cloudflared` missing, failing to
//! start, ending, or printing no address within a bounded wait. Stopping or dropping the tunnel
//! ends the process.

mod address;
mod failure;
mod program;

pub use failure::TunnelFailure;
pub use program::TunnelProgram;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use agent_fork::{Exit, ForkCommand, ForkOutput, Launcher, RunningFork};

/// What a tunnel reports, once each, in this order: the public address, then its end; or only
/// why it gave no address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TunnelEvent {
    /// The tunnel's public host name, such as `quiet-river-stone-lamp.trycloudflare.com`: the
    /// tunnel forwards `https://` requests for it to the local address.
    Opened { host: String },
    /// The tunnel gave no address, or ended after it gave one. Nothing follows.
    Failed(TunnelFailure),
}

/// A running quick tunnel. Stopping or dropping it ends `cloudflared`, whose end is not reported.
pub struct QuickTunnel {
    fork: Arc<RunningFork>,
    /// Set once the tunnel is stopped: no event is reported after.
    stopped: Arc<AtomicBool>,
    /// The thread `cloudflared` started from, whose end sends it the parent-death signal.
    _launcher: Launcher,
}

impl QuickTunnel {
    /// Starts `program`'s tunnel to `target`, a local HTTP address. `events` receives, on a
    /// thread of the tunnel's, the public host name, then the tunnel's end; or why there is no
    /// address: the process ended first, or printed none within the program's wait, after which
    /// the tunnel stops it. Fails at once when the program is missing or cannot start.
    pub fn start(
        program: &TunnelProgram,
        target: SocketAddr,
        events: impl FnMut(TunnelEvent) + Send + 'static,
    ) -> Result<Self, TunnelFailure> {
        let executable = program.find()?;
        let launcher = Launcher::start(program.wrapper.clone());
        let (seen, sightings) = mpsc::channel();
        let fork = launcher
            .spawn(
                ForkCommand {
                    program: executable,
                    arguments: TunnelProgram::arguments(target),
                    directory: program.directory.clone(),
                    environment: program.environment.clone(),
                },
                String::new(),
                Box::new(Watch { seen, found: false }),
            )
            .map_err(|error| TunnelFailure::NotStarted(error.to_string()))?;
        let fork = Arc::new(fork);
        let stopped = Arc::new(AtomicBool::new(false));
        let supervisor = Supervisor {
            sightings,
            fork: Arc::clone(&fork),
            wait: program.address_wait,
            stopped: Arc::clone(&stopped),
        };
        thread::Builder::new()
            .name("quick tunnel".into())
            .spawn(move || supervisor.run(events))
            .map_err(|error| {
                fork.terminate();
                TunnelFailure::NotStarted(error.to_string())
            })?;
        Ok(Self {
            fork,
            stopped,
            _launcher: launcher,
        })
    }

    /// Ends `cloudflared`: SIGTERM, then SIGKILL three seconds later. Returns once it ended, at
    /// most five seconds later. Its end is not reported; an event that the tunnel was reporting
    /// as this started may still arrive.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.fork.terminate();
    }
}

impl Drop for QuickTunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Follows the tunnel's process on a thread of its own, and reports what it showed.
struct Supervisor {
    sightings: mpsc::Receiver<Sighting>,
    fork: Arc<RunningFork>,
    /// How long the process has to print the address.
    wait: Duration,
    /// Set once the tunnel is stopped: nothing is reported after.
    stopped: Arc<AtomicBool>,
}

impl Supervisor {
    /// Reports the public host name, then the end of the process; or why there is no address.
    fn run(self, mut events: impl FnMut(TunnelEvent)) {
        let mut report = |event| {
            if !self.stopped.load(Ordering::SeqCst) {
                events(event);
            }
        };
        match self.address() {
            Ok(host) => {
                report(TunnelEvent::Opened { host });
                report(TunnelEvent::Failed(TunnelFailure::Ended(self.end())));
            }
            Err(failure) => report(TunnelEvent::Failed(failure)),
        }
    }

    /// The public host name, once the process printed it; or why it gave none: it ended first,
    /// or printed none within the wait, after which it is stopped.
    fn address(&self) -> Result<String, TunnelFailure> {
        match self.sightings.recv_timeout(self.wait) {
            Ok(Sighting::Host(host)) => Ok(host),
            Ok(Sighting::End(exit)) => Err(TunnelFailure::Exited(exit)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.fork.terminate();
                Err(TunnelFailure::NoAddress(self.wait))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(TunnelFailure::Exited(Exit::default()))
            }
        }
    }

    /// How the process ended, once it did. The process reports its address once, so its end
    /// is what follows.
    fn end(&self) -> Exit {
        match self.sightings.recv() {
            Ok(Sighting::End(exit)) => exit,
            Ok(Sighting::Host(_)) | Err(_) => Exit::default(),
        }
    }
}

/// What the tunnel's process showed: its public host name, or its end.
enum Sighting {
    Host(String),
    End(Exit),
}

/// Reads `cloudflared`'s output for the public address, which it prints in its log on the
/// standard error; the standard output is read too.
struct Watch {
    seen: mpsc::Sender<Sighting>,
    /// The address was found: later lines are not read for it.
    found: bool,
}

impl Watch {
    fn read(&mut self, line: &str) {
        if self.found {
            return;
        }
        if let Some(host) = address::quick_tunnel_host(line) {
            self.found = true;
            let _ = self.seen.send(Sighting::Host(host.to_owned()));
        }
    }
}

impl ForkOutput for Watch {
    fn line(&mut self, line: &str) {
        self.read(line);
    }

    fn error_line(&mut self, line: &str) {
        self.read(line);
    }

    fn ended(self: Box<Self>, exit: Exit) {
        let _ = self.seen.send(Sighting::End(exit));
    }
}

#[cfg(test)]
mod tests;
