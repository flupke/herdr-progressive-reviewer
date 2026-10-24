//! [`Follow`]: a client that joined a session of the daemon, and what it hears of it.

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tungstenite::{Message, WebSocket};

use crate::wire::{self, Incoming, Notice};

/// What a [`Follow`] hears of its session, in the order the daemon said it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionEvent {
    /// The client joined the session, first or again once the daemon came back, and the
    /// session's status then, when the daemon gave one the client reads.
    Joined(Option<SessionStatus>),
    /// The daemon refused the first join, in its words, such as a session with no rollout yet,
    /// which had no prompt: the follow ends. A refused join again, once the daemon came back, is
    /// [`SessionEvent::Lost`] instead, and the client tries again.
    Refused(String),
    /// The client lost the daemon, could not reach it, or was refused a join again: it tries
    /// again until it joins.
    Lost(String),
    TurnStarted {
        turn: String,
    },
    /// The prompt `text` the turn `turn` started on, as the session took it.
    Prompt {
        turn: String,
        text: String,
    },
    TurnEnded {
        turn: String,
        end: TurnEnd,
    },
    Status(SessionStatus),
    /// Another client joined the session: the agent's TUI resumed it with `/resume`, or once it
    /// reconnected to a restarted daemon after the client connected again. The client's own
    /// joins are not reported.
    Resumed,
}

/// A session's status, as the daemon reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionStatus {
    /// The daemon holds the session no more.
    NotLoaded,
    Idle,
    /// A turn runs, or waits for an approval.
    Active,
    SystemError,
}

/// How a turn ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnEnd {
    Completed,
    Interrupted,
    Failed,
}

/// Follows a session of the daemon on a thread of its own: dropping it stops the thread, and
/// waits for it, so its `report` must not drop it.
#[must_use = "dropping the follow stops it"]
pub struct Follow {
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Follow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Follow")
    }
}

/// Stops a follow's thread: wakes it while it waits to connect again, and shuts the socket it
/// reads down.
#[derive(Default)]
struct Stop {
    state: Mutex<StopState>,
    woken: Condvar,
}

#[derive(Default)]
struct StopState {
    stopped: bool,
    socket: Option<UnixStream>,
}

impl Follow {
    pub(crate) fn start(
        socket: PathBuf,
        session: String,
        retry: Duration,
        mut report: impl FnMut(SessionEvent) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(Stop::default());
        let thread = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || run(&socket, &session, retry, &stop, &mut report))
        };
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Follow {
    fn drop(&mut self) {
        let mut state = self.stop.lock();
        state.stopped = true;
        if let Some(socket) = state.socket.take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        drop(state);
        self.stop.woken.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Stop {
    fn lock(&self) -> std::sync::MutexGuard<'_, StopState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn stopped(&self) -> bool {
        self.lock().stopped
    }

    /// Keeps a handle of `stream`, which the thread reads now, to shut it down on a stop; fails
    /// once stopped.
    fn reading(&self, stream: &UnixStream) -> Result<(), ()> {
        let mut state = self.lock();
        if state.stopped {
            return Err(());
        }
        state.socket = stream.try_clone().ok();
        Ok(())
    }

    /// Waits `retry`, unless stopped first: returns whether the thread goes on.
    fn wait(&self, retry: Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .woken
            .wait_timeout_while(state, retry, |state| !state.stopped)
            .unwrap_or_else(PoisonError::into_inner);
        !state.stopped
    }
}

/// Why a connection to the daemon ended.
enum Ended {
    Refused(String),
    Lost(String),
}

/// Where a follow stands across its connections to the daemon.
#[derive(Default)]
struct Progress {
    /// The client joined the session once.
    joined: bool,
    /// The client reported the loss of the daemon and has not joined since.
    lost: bool,
}

/// What a follow does once a connection ended.
#[derive(Debug, Eq, PartialEq)]
enum Next {
    /// Reports the event, if any, and connects again after the retry delay.
    Retry(Option<SessionEvent>),
    /// Reports the event and stops.
    Stop(SessionEvent),
}

impl Progress {
    fn joined(&mut self) {
        *self = Self {
            joined: true,
            lost: false,
        };
    }

    /// What follows the end `ended` of a connection: a refusal of the first join ends the
    /// follow, and any other end is a loss of the daemon, reported once until the client joins
    /// again.
    fn ended(&mut self, ended: Ended) -> Next {
        match ended {
            Ended::Refused(error) if !self.joined => Next::Stop(SessionEvent::Refused(error)),
            Ended::Refused(error) => self.lose(format!(
                "the Codex daemon refused to join the session again: {error}"
            )),
            Ended::Lost(error) => self.lose(error),
        }
    }

    fn lose(&mut self, error: String) -> Next {
        Next::Retry((!std::mem::replace(&mut self.lost, true)).then_some(SessionEvent::Lost(error)))
    }
}

/// Follows `session` until stopped, or refused on the first join: joins it over each
/// connection to the daemon at `socket`, every `retry` while it cannot, and reports a loss of
/// the daemon once until it joins again.
fn run(
    socket: &Path,
    session: &str,
    retry: Duration,
    stop: &Stop,
    report: &mut impl FnMut(SessionEvent),
) {
    let mut progress = Progress::default();
    loop {
        let ended = match UnixStream::connect(socket) {
            Ok(stream) if stop.reading(&stream).is_err() => return,
            Ok(stream) => follow(stream, session, &mut progress, report),
            Err(error) => Ended::Lost(format!(
                "cannot reach the Codex daemon at {}: {error}",
                socket.display()
            )),
        };
        match progress.ended(ended) {
            Next::Stop(event) => return report(event),
            Next::Retry(_) if stop.stopped() => return,
            Next::Retry(event) => event.into_iter().for_each(&mut *report),
        }
        if !stop.wait(retry) {
            return;
        }
    }
}

/// Joins `session` over `stream` and reports what the daemon says of it, until the connection
/// ends.
fn follow(
    stream: UnixStream,
    session: &str,
    progress: &mut Progress,
    report: &mut impl FnMut(SessionEvent),
) -> Ended {
    let mut socket = match tungstenite::client("ws://localhost/", stream) {
        Ok((socket, _)) => socket,
        Err(error) => return lose(&error),
    };
    if let Err(error) = socket.send(Message::text(wire::initialize())) {
        return lose(&error);
    }
    let mut heard = Hearing::joining(session);
    loop {
        let incoming = match next_text(&mut socket) {
            Ok(text) => wire::read(&text),
            Err(error) => return lose(&error),
        };
        if let Err(ended) = answer(&mut socket, session, &incoming) {
            return ended;
        }
        if let Some(event) = heard.hear(incoming) {
            if let SessionEvent::Joined(_) = event {
                progress.joined();
            }
            report(event);
        }
    }
}

/// Sends what the daemon's answer `incoming` calls for: the join, once the daemon took the
/// client.
fn answer(
    socket: &mut WebSocket<UnixStream>,
    session: &str,
    incoming: &Incoming,
) -> Result<(), Ended> {
    let Incoming::Answer { id, result } = incoming else {
        return Ok(());
    };
    match (*id, result) {
        (wire::INITIALIZE, Ok(_)) => socket
            .send(Message::text(wire::initialized()))
            .and_then(|()| socket.send(Message::text(wire::join(session))))
            .map_err(|error| lose(&error)),
        (wire::INITIALIZE, Err(error)) => Err(lose(error)),
        (wire::JOIN, Err(error)) => Err(Ended::Refused(error.clone())),
        _ => Ok(()),
    }
}

fn lose(error: &dyn std::fmt::Display) -> Ended {
    Ended::Lost(format!("lost the Codex daemon: {error}"))
}

/// The next text frame of `socket`, after the pings it answers.
fn next_text(socket: &mut WebSocket<UnixStream>) -> Result<String, tungstenite::Error> {
    loop {
        match socket.read()? {
            Message::Text(text) => return Ok(text.as_str().to_owned()),
            Message::Close(_) => return Err(tungstenite::Error::ConnectionClosed),
            _ => {}
        }
    }
}

/// What the client makes of what the daemon says once it asked to join a session.
struct Hearing<'a> {
    session: &'a str,
    /// The goal snapshots still to come that the client's own join causes.
    own_snapshots: usize,
}

impl<'a> Hearing<'a> {
    /// A session the client asked to join.
    fn joining(session: &'a str) -> Self {
        Self {
            session,
            own_snapshots: 1,
        }
    }

    /// What `incoming` says of the session, if anything.
    fn hear(&mut self, incoming: Incoming) -> Option<SessionEvent> {
        match incoming {
            Incoming::Answer {
                id: wire::JOIN,
                result: Ok(status),
            } => Some(SessionEvent::Joined(status)),
            Incoming::Notice { session, notice } if session == self.session => match notice {
                Notice::Event(event) => Some(event),
                Notice::GoalSnapshot if self.own_snapshots > 0 => {
                    self.own_snapshots -= 1;
                    None
                }
                Notice::GoalSnapshot => Some(SessionEvent::Resumed),
            },
            Incoming::Answer { .. } | Incoming::Notice { .. } | Incoming::Other => None,
        }
    }
}

#[cfg(test)]
#[path = "follow.tests.rs"]
mod tests;
