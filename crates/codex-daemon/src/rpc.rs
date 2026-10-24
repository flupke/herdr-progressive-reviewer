//! [`Rpc`]: a client of the daemon that makes requests, hears notifications and answers the
//! daemon's own requests, such as the approvals a turn of a session it started waits for.

use std::collections::HashMap;
use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

/// How long the connection waits for the daemon's answer to its handshake.
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(10);

/// How long the connection's thread reads before it sends what waits to be sent.
const READ_SLICE: Duration = Duration::from_millis(20);

/// What the daemon says to a client, besides the answers to its requests.
#[derive(Debug)]
pub enum Incoming {
    /// A notification, such as `turn/completed` of a session the client started or joined.
    Notification { method: String, params: Value },
    /// A request the client answers with [`Replier::reply`], such as the approval of a tool
    /// call in a turn of a session it started.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// The connection ended, in these words: nothing more comes.
    Lost(String),
}

/// Answers the daemon's requests.
#[derive(Clone)]
pub struct Replier {
    outgoing: Sender<String>,
}

impl Replier {
    /// Answers the daemon's request `id` with `result`, or with the error `message`.
    pub fn reply(&self, id: &Value, result: Result<Value, String>) {
        let message = match result {
            Ok(result) => json!({"id": id, "result": result}),
            Err(message) => json!({"id": id, "error": {"code": -32_000, "message": message}}),
        };
        let _ = self.outgoing.send(message.to_string());
    }
}

/// A connection to the daemon, on a thread of its own that hands what the daemon says to the
/// connection's handler. Dropping it closes the connection.
pub struct Rpc {
    outgoing: Sender<String>,
    pending: Arc<Pending>,
    next: AtomicU64,
    stopped: Arc<AtomicBool>,
    socket: UnixStream,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Rpc {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Rpc")
    }
}

/// The requests of the client that wait for their answer, by ID.
type Pending = Mutex<HashMap<u64, Sender<Result<Value, String>>>>;

impl Rpc {
    /// Connects to the daemon at `socket`, as a client that knows the experimental API, and
    /// hands `handler` what the daemon says from then on, in order, on the connection's thread.
    pub(crate) fn connect(
        socket: &Path,
        handler: impl FnMut(Incoming, &Replier) + Send + 'static,
    ) -> Result<Self, String> {
        let stream = UnixStream::connect(socket).map_err(|error| {
            format!(
                "cannot reach the Codex daemon at {}: {error}",
                socket.display()
            )
        })?;
        let lost = |error: &dyn std::fmt::Display| format!("lost the Codex daemon: {error}");
        let shutdown = stream.try_clone().map_err(|error| lost(&error))?;
        stream
            .set_read_timeout(Some(HANDSHAKE_LIMIT))
            .map_err(|error| lost(&error))?;
        let (mut websocket, _) =
            tungstenite::client("ws://localhost/", stream).map_err(|error| lost(&error))?;
        handshake(&mut websocket).map_err(|error| lost(&error))?;
        websocket
            .get_ref()
            .set_read_timeout(Some(READ_SLICE))
            .map_err(|error| lost(&error))?;
        let (outgoing, sent) = mpsc::channel();
        let pending = Arc::new(Pending::default());
        let stopped = Arc::new(AtomicBool::new(false));
        let thread = {
            let replier = Replier {
                outgoing: outgoing.clone(),
            };
            let (pending, stopped) = (Arc::clone(&pending), Arc::clone(&stopped));
            thread::spawn(move || run(websocket, &sent, &replier, &pending, &stopped, handler))
        };
        Ok(Self {
            outgoing,
            pending,
            next: AtomicU64::new(1),
            stopped,
            socket: shutdown,
            thread: Some(thread),
        })
    }

    /// Makes the request `method` with `params`, and returns its result once the daemon
    /// answers, or the daemon's error; gives up after `limit`.
    pub fn request(&self, method: &str, params: &Value, limit: Duration) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = mpsc::channel();
        lock(&self.pending).insert(id, answer);
        let message = json!({"id": id, "method": method, "params": params}).to_string();
        if self.outgoing.send(message).is_err() {
            lock(&self.pending).remove(&id);
            return Err("lost the Codex daemon".into());
        }
        let result = answered.recv_timeout(limit);
        lock(&self.pending).remove(&id);
        result.unwrap_or_else(|_| {
            Err(format!(
                "the Codex daemon did not answer {method} within {} s",
                limit.as_secs_f64()
            ))
        })
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.socket.shutdown(Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(
    pending: &Pending,
) -> std::sync::MutexGuard<'_, HashMap<u64, Sender<Result<Value, String>>>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `initialize`, its answer, then `initialized`, as the daemon expects of every client.
fn handshake(websocket: &mut WebSocket<UnixStream>) -> Result<(), String> {
    websocket
        .send(Message::text(crate::wire::initialize()))
        .map_err(|error| error.to_string())?;
    loop {
        let Message::Text(text) = websocket.read().map_err(|error| error.to_string())? else {
            continue;
        };
        let message: Value = serde_json::from_str(text.as_str()).unwrap_or_default();
        if message.get("method").is_some() {
            continue;
        }
        if message.get("id").and_then(Value::as_u64) == Some(crate::wire::INITIALIZE) {
            if let Some(error) = message.pointer("/error/message").and_then(Value::as_str) {
                return Err(error.to_owned());
            }
            break;
        }
    }
    websocket
        .send(Message::text(crate::wire::initialized()))
        .map_err(|error| error.to_string())
}

/// Reads what the daemon says and sends what waits to be sent, until the connection ends or
/// the client stops; then tells `handler` and the requests that wait.
fn run(
    mut websocket: WebSocket<UnixStream>,
    sent: &Receiver<String>,
    replier: &Replier,
    pending: &Pending,
    stopped: &AtomicBool,
    mut handler: impl FnMut(Incoming, &Replier),
) {
    let ended = loop {
        if stopped.load(Ordering::Acquire) {
            return;
        }
        match read_once(&mut websocket, pending) {
            Ok(Some(incoming)) => handler(incoming, replier),
            Ok(None) => {}
            Err(ended) => break ended,
        }
        if let Err(error) = send_waiting(&mut websocket, sent) {
            break format!("lost the Codex daemon: {error}");
        }
    };
    for (_, answer) in lock(pending).drain() {
        let _ = answer.send(Err(ended.clone()));
    }
    if !stopped.load(Ordering::Acquire) {
        handler(Incoming::Lost(ended), replier);
    }
}

/// What the daemon said within a read's slice of time, once the answer it may be went to the
/// request that waits for it in `pending`; why the connection ended, when it did.
fn read_once(
    websocket: &mut WebSocket<UnixStream>,
    pending: &Pending,
) -> Result<Option<Incoming>, String> {
    match websocket.read() {
        Ok(Message::Text(text)) => Ok(hear(text.as_str(), pending)),
        Ok(Message::Close(_)) => Err("the Codex daemon closed the connection".to_owned()),
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Io(error))
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(format!("lost the Codex daemon: {error}")),
    }
}

/// Sends every message that waits to be sent.
fn send_waiting(
    websocket: &mut WebSocket<UnixStream>,
    sent: &Receiver<String>,
) -> Result<(), tungstenite::Error> {
    while let Ok(message) = sent.try_recv() {
        websocket.send(Message::text(message))?;
    }
    Ok(())
}

/// What the text frame `text` says, once the answer it may be went to the request that waits
/// for it in `pending`.
fn hear(text: &str, pending: &Pending) -> Option<Incoming> {
    let mut message: Value = serde_json::from_str(text).ok()?;
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let params = message
        .get_mut("params")
        .map(Value::take)
        .unwrap_or_default();
    match (method, message.get("id").cloned()) {
        (Some(method), Some(id)) => Some(Incoming::Request { id, method, params }),
        (Some(method), None) => Some(Incoming::Notification { method, params }),
        (None, Some(id)) => {
            let answer = lock(pending).remove(&id.as_u64()?)?;
            let result = match message.get("error") {
                Some(error) => Err(error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("the Codex daemon refused the request")
                    .to_owned()),
                None => Ok(message
                    .get_mut("result")
                    .map(Value::take)
                    .unwrap_or_default()),
            };
            let _ = answer.send(result);
            None
        }
        (None, None) => None,
    }
}
