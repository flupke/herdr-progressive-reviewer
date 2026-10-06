//! The event socket of the tests' stand-ins for an agent and its forks.
//!
//! A test listens on a Unix socket with [`StandInEvents::listen`] and names it to the
//! stand-ins it starts in [`EVENTS_VARIABLE`]. Each stand-in process connects with
//! [`StandInConnection::from_env`], says who it is, then writes one JSON line per
//! [`StandInEvent`]; the test sends it [`StandInCommand`]s on the same connection. The test waits
//! on what the stand-ins report instead of polling files, and proves that something did not
//! happen by ordering: what a stand-in did before it reported a later event, it reported
//! before that event.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::thread;

use serde::{Deserialize, Serialize};

/// The environment variable that names the socket to the stand-ins.
pub const EVENTS_VARIABLE: &str = "REVIEW_STAND_IN_EVENTS";

/// The prompt that the stand-in agent reads as the marker `id`: it reports
/// [`StandInEvent::Marker`] and does nothing else with it.
pub fn marker_prompt(id: u64) -> String {
    format!("::stand-in-marker {id}")
}

/// The marker a prompt is, if it is one.
pub fn marker_of(prompt: &str) -> Option<u64> {
    prompt
        .trim()
        .strip_prefix("::stand-in-marker ")?
        .parse()
        .ok()
}

/// Which stand-in a connection is.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum StandInRole {
    /// The stand-in agent in the agent's pane.
    Agent,
    /// A second stand-in agent, in a pane of its own.
    SecondAgent,
    /// A stand-in fork, which takes the session `session`.
    Fork { session: String },
}

/// What a stand-in reports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StandInEvent {
    /// The first line of a connection: who the stand-in is.
    Hello { role: StandInRole },
    /// The agent read a prompt submitted in its pane: neither a `/resume` nor a marker.
    PromptReceived { text: String },
    /// The agent read Claude Code's own `/resume <session>`.
    ResumeReceived { session: String },
    /// The agent reported its state to Herdr: `idle` or `working`.
    StateReported { state: String },
    /// The agent reported its session to Herdr as Claude Code's session hook does, from
    /// `start`: `startup` or `resume`.
    SessionReported { session: String, start: String },
    /// Herdr released the agent, whose state it reads from its title from now on, or the test
    /// said it did.
    Released,
    /// The agent started a turn on a prompt, and shows it working.
    TurnStarted,
    /// The agent ended its turn, and shows the title the test gave it.
    TurnFinished,
    /// The agent ends each turn only when the test ends it (`hold`), or as soon as Herdr saw
    /// it start, from now on.
    TurnsHeld { hold: bool },
    /// The agent read the marker `id` in its pane: it read what Herdr wrote there before.
    Marker { id: u64 },
    /// The agent exits, on Ctrl-D.
    Exited,
    /// A fork started on `prompt`, from the session `parent`.
    ForkStarted { parent: String, prompt: String },
    /// The fork submitted its turn, and printed the answer to its submit.
    ForkSubmitted,
    /// The fork ended its turn by itself.
    ForkFinished,
    /// The connection closed: the stand-in's process ended, however it ended. The test side
    /// reports it; no stand-in writes it.
    Disconnected,
}

/// What a test tells a stand-in.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum StandInCommand {
    /// Agent: end each turn only on [`StandInCommand::EndTurn`] (`hold`), or as soon as Herdr
    /// saw it start (the default).
    HoldTurns { hold: bool },
    /// Agent: end the turn under way.
    EndTurn,
    /// Agent: Herdr reads your title from now on, since the test released you. Herdr says so
    /// only when it did not detect the agent by name yet.
    Release,
    /// Agent: show `title`, which Herdr's detection reads, outside its turns.
    ShowTitle { title: String },
    /// Agent: show `text` as its screen.
    ShowScreen { text: String },
    /// Fork: submit its turn.
    Submit,
    /// Fork: end its turn, and exit.
    End,
}

/// One event, and the stand-in that reported it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reported {
    pub from: StandInRole,
    pub event: StandInEvent,
}

/// The stand-in's end of the socket.
pub struct StandInConnection {
    writer: Mutex<UnixStream>,
}

impl StandInConnection {
    /// Connects to the socket the test named in [`EVENTS_VARIABLE`] as `role`, and returns the
    /// connection and the commands the test sends; `None` when the test named none.
    pub fn from_env(role: StandInRole) -> Option<(Self, mpsc::Receiver<StandInCommand>)> {
        Self::connect(Path::new(&std::env::var_os(EVENTS_VARIABLE)?), role)
    }

    /// Connects to the socket at `path` as `role`, and returns the connection and the commands
    /// the test sends; `None` when nothing listens there.
    pub(crate) fn connect(
        path: &Path,
        role: StandInRole,
    ) -> Option<(Self, mpsc::Receiver<StandInCommand>)> {
        let stream = UnixStream::connect(path).ok()?;
        let reader = BufReader::new(stream.try_clone().ok()?);
        let (sender, commands) = mpsc::channel();
        thread::spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else {
                    return;
                };
                let command = serde_json::from_str(&line).expect("a stand-in command");
                if sender.send(command).is_err() {
                    return;
                }
            }
        });
        let connection = Self {
            writer: Mutex::new(stream),
        };
        connection.report(&StandInEvent::Hello { role });
        Some((connection, commands))
    }

    /// Reports `event` to the test.
    pub fn report(&self, event: &StandInEvent) {
        let mut line = serde_json::to_vec(event).unwrap();
        line.push(b'\n');
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        // A test that already ended no longer reads.
        let _ = writer.write_all(&line);
    }
}

impl Drop for StandInConnection {
    /// Closes the connection, which the thread reading the test's commands also holds.
    fn drop(&mut self) {
        let writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writer.shutdown(std::net::Shutdown::Both);
    }
}

/// The connections of the stand-ins, by role, as they connect: a role that connects again
/// replaces its earlier connection.
#[derive(Default)]
struct Connections {
    /// The connection of each role, and its number.
    writers: Mutex<BTreeMap<StandInRole, (u64, UnixStream)>>,
    connected: Condvar,
    next: AtomicU64,
}

/// The test's end of the socket: every stand-in's events, in the order each reported them.
///
/// It keeps every event it received, so that a test may wait on the events in turn
/// ([`Self::events_until`]) and on what all of them say together ([`Self::wait_until`]).
pub struct StandInEvents {
    path: PathBuf,
    /// Each event, or why a stand-in's line was not one.
    events: Mutex<mpsc::Receiver<Result<Reported, String>>>,
    /// Every event received, and how many of them [`Self::events_until`] went through.
    received: Mutex<Received>,
    connections: Arc<Connections>,
    markers: AtomicU64,
}

#[derive(Default)]
struct Received {
    events: Vec<Reported>,
    read: usize,
}

impl StandInEvents {
    /// Listens at `path`, under the test's private directory.
    pub fn listen(path: &Path) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        let (sender, events) = mpsc::channel();
        let connections = Arc::new(Connections::default());
        let accepted = Arc::clone(&connections);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                let (sender, connections) = (sender.clone(), Arc::clone(&accepted));
                thread::spawn(move || read_connection(&stream, &sender, &connections));
            }
        });
        Self {
            path: path.to_owned(),
            events: Mutex::new(events),
            received: Mutex::default(),
            connections,
            markers: AtomicU64::new(0),
        }
    }

    /// The environment variable, `NAME=value`, that names this socket to a stand-in.
    pub fn environment(&self) -> String {
        format!("{EVENTS_VARIABLE}={}", self.path.display())
    }

    /// The next event that `matches`, skipping the others; fails after [`crate::GUARD`], when
    /// none came, with `what` it waited for.
    pub fn wait_for(&self, what: &str, matches: impl FnMut(&Reported) -> bool) -> Reported {
        self.events_until(what, matches).pop().unwrap()
    }

    /// Every event after those an earlier call went through, up to and including the first
    /// that `matches`; fails after [`crate::GUARD`], when none came, with `what` it waited for.
    pub fn events_until(
        &self,
        what: &str,
        mut matches: impl FnMut(&Reported) -> bool,
    ) -> Vec<Reported> {
        let mut checked = self.lock_received().read;
        loop {
            let mut received = self.lock_received();
            if let Some(found) = received.events[checked..].iter().position(&mut matches) {
                let end = checked + found + 1;
                let events = received.events[received.read..end].to_vec();
                received.read = end;
                return events;
            }
            checked = received.events.len();
            drop(received);
            self.receive(what);
        }
    }

    /// Every event received, once `holds` holds for them; fails after [`crate::GUARD`], when
    /// it does not, with `what` it waited for.
    pub fn wait_until(
        &self,
        what: &str,
        mut holds: impl FnMut(&[Reported]) -> bool,
    ) -> Vec<Reported> {
        loop {
            let received = self.lock_received();
            if holds(&received.events) {
                return received.events.clone();
            }
            drop(received);
            self.receive(what);
        }
    }

    /// Every event received so far.
    pub fn received(&self) -> Vec<Reported> {
        self.lock_received().events.clone()
    }

    /// Receives the next event; fails after [`crate::GUARD`], when none came, with `what` the
    /// test waited for.
    fn receive(&self, what: &str) {
        let next = self
            .events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .recv_timeout(crate::GUARD);
        let mut received = self.lock_received();
        let before = &received.events[received.read..];
        let event = next
            .unwrap_or_else(|error| {
                panic!("no stand-in reported {what} ({error}); since the last wait: {before:#?}")
            })
            .unwrap_or_else(|error| panic!("{error}; since the last wait: {before:#?}"));
        received.events.push(event);
    }

    fn lock_received(&self) -> std::sync::MutexGuard<'_, Received> {
        self.received.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Submits a new marker with `submit`, which types the prompt it gets into the agent's
    /// pane, and returns every event up to the agent's report of it: what the agent did with
    /// what Herdr wrote in its pane before the marker.
    pub fn mark(&self, submit: impl FnOnce(&str)) -> Vec<Reported> {
        let id = self.markers.fetch_add(1, Ordering::Relaxed);
        submit(&marker_prompt(id));
        self.events_until(&format!("the marker {id}"), |reported| {
            reported.event == StandInEvent::Marker { id }
        })
    }

    /// Sends `command` to the stand-in of `role`, once it connected; fails after
    /// [`crate::GUARD`] when it does not.
    pub fn send(&self, role: &StandInRole, command: &StandInCommand) {
        let mut line = serde_json::to_vec(command).unwrap();
        line.push(b'\n');
        let writers = self
            .connections
            .writers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (mut writers, timeout) = self
            .connections
            .connected
            .wait_timeout_while(writers, crate::GUARD, |writers| !writers.contains_key(role))
            .unwrap_or_else(PoisonError::into_inner);
        assert!(
            !timeout.timed_out(),
            "the stand-in {role:?} did not connect"
        );
        writers
            .get_mut(role)
            .unwrap()
            .1
            .write_all(&line)
            .unwrap_or_else(|error| panic!("the stand-in {role:?} is gone: {error}"));
    }
}

/// Reads the events of one stand-in into `sender`, then reports that it disconnected.
fn read_connection(
    stream: &UnixStream,
    sender: &mpsc::Sender<Result<Reported, String>>,
    connections: &Connections,
) {
    let number = connections.next.fetch_add(1, Ordering::Relaxed);
    let mut role = None;
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        let event: StandInEvent = match serde_json::from_str(&line) {
            Ok(event) => event,
            Err(error) => {
                let _ = sender.send(Err(format!("a stand-in wrote {line:?}: {error}")));
                break;
            }
        };
        if let StandInEvent::Hello { role: hello } = &event {
            connections
                .writers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(hello.clone(), (number, stream.try_clone().unwrap()));
            connections.connected.notify_all();
            role = Some(hello.clone());
        }
        let Some(from) = role.clone() else {
            let _ = sender.send(Err(format!(
                "a stand-in reported {event:?} before it said who it is"
            )));
            break;
        };
        let _ = sender.send(Ok(Reported { from, event }));
    }
    if let Some(from) = role {
        let mut writers = connections
            .writers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // A stand-in of the same role that connected since keeps its connection.
        if writers
            .get(&from)
            .is_some_and(|(owner, _)| *owner == number)
        {
            writers.remove(&from);
        }
        drop(writers);
        let _ = sender.send(Ok(Reported {
            from,
            event: StandInEvent::Disconnected,
        }));
    }
}

#[cfg(test)]
#[path = "stand_in.tests.rs"]
mod tests;
