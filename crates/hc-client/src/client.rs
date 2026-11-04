//! herdr-core's JSON-RPC socket, one message per line.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use herdr_client::client::{EventCanceller, Tie, read_line, write_json_line};
use herdr_client::protocol::{
    Agent, AgentHost, AgentPort, AgentStatus, HerdrEvent, PaneId, PaneProcessInfo, SessionReport,
    SessionSnapshot,
};
use herdr_client::{Error, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::events::{Event, Roster, StatusChange, status_of};
use crate::terminal::Terminal;

const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `terminal.send` may take: it waits until nobody typed in the terminal for about
/// 3 seconds, then a few seconds more to tell whether the program went working.
const SEND_TIMEOUT: Duration = Duration::from_secs(60);
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A synchronous client of herdr-core, the core of herdr-app that runs its terminals and
/// agents. herdr-core keeps no focus and no agent session: the agents it gives have none.
#[derive(Clone, Debug)]
pub struct HcClient {
    socket_path: PathBuf,
}

/// A refusal of herdr-core: `{code, message, data: {kind, hint}}`.
#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(default)]
    data: Option<ErrorData>,
}

#[derive(Debug, Deserialize)]
struct ErrorData {
    /// Such as `not_found`, `blocked` or `finished`.
    #[serde(default)]
    kind: Option<String>,
}

impl ApiError {
    fn is(&self, kind: &str) -> bool {
        self.data
            .as_ref()
            .and_then(|data| data.kind.as_deref())
            .is_some_and(|actual| actual == kind)
    }
}

/// One line from herdr-core: the answer to a request, or a notification.
#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ApiError>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
}

/// What `terminal.send` answers (`SendResult` in `hc schema`).
#[derive(Debug, Deserialize)]
struct Sent {
    /// Whether the program went working within a few seconds; `None` when it reports no
    /// status.
    #[serde(default)]
    started: Option<bool>,
    /// The program's status when the text went in.
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Listed {
    terminals: Vec<Terminal>,
}

/// The snapshot `events.subscribe` answers, before the events that follow it.
#[derive(Debug, Deserialize)]
struct Snapshot {
    terminals: Vec<Terminal>,
}

/// The events of one subscription, in the order herdr-core sent them.
struct Subscription {
    reader: BufReader<UnixStream>,
    /// Ties the stream to what ends it: a read it ends is no error.
    tie: Tie,
    socket_path: PathBuf,
}

impl HcClient {
    /// A client of the core whose socket is `socket_path`.
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// A client of the core of the terminal this process runs in: herdr-core gives each of its
    /// terminals its socket in `HC_TERMINAL_SOCKET`.
    pub fn from_env() -> Result<Self> {
        std::env::var_os("HC_TERMINAL_SOCKET")
            .filter(|path| !path.is_empty())
            .map(|path| Self::new(path.into()))
            .ok_or(Error::Environment {
                name: "HC_TERMINAL_SOCKET",
            })
    }

    /// The labels of `terminal`.
    pub fn labels(&self, terminal: &PaneId) -> Result<BTreeMap<String, String>> {
        let terminal = self.terminal(terminal)?.ok_or_else(|| Error::Herdr {
            operation: "terminal.get",
            message: format!("no terminal {:?}", terminal.0),
        })?;
        Ok(terminal.labels)
    }

    /// Sets `labels` on `terminal`; a label set to an empty value is removed. herdr-core tells
    /// each subscriber of the terminal's events.
    pub fn set_labels(&self, terminal: &PaneId, labels: &BTreeMap<String, String>) -> Result<()> {
        self.call::<Value>(
            "terminal.label",
            &json!({"terminal": terminal.0, "labels": labels}),
        )
        .map(|_| ())
    }

    /// The terminal at `address`, live or finished; `None` when herdr-core has none there.
    fn terminal(&self, address: &PaneId) -> Result<Option<Terminal>> {
        match self.response(
            "terminal.get",
            &json!({"terminal": address.0}),
            SOCKET_TIMEOUT,
        )? {
            Ok(result) => parse(result, "terminal.get").map(Some),
            Err(error) if error.is("not_found") => Ok(None),
            Err(error) => Err(refusal("terminal.get", error)),
        }
    }

    /// Pastes `text` into the terminal of `pane_id` and presses Enter. Sent without a caller,
    /// so herdr-core types the text as it is, without the `<hc from="...">` tags it puts around
    /// what one of its terminals sends another.
    fn send(&self, pane_id: &PaneId, text: &str) -> Result<Sent> {
        match self.response(
            "terminal.send",
            &json!({"terminal": pane_id.0, "text": text}),
            SEND_TIMEOUT,
        )? {
            Ok(result) => parse(result, "terminal.send"),
            Err(error) => Err(refusal("terminal.send", error)),
        }
    }

    /// Calls `method` with `params`, and parses its result; herdr-core's refusal is an error.
    fn call<T: DeserializeOwned>(&self, method: &'static str, params: &Value) -> Result<T> {
        match self.response(method, params, SOCKET_TIMEOUT)? {
            Ok(result) => parse(result, method),
            Err(error) => Err(refusal(method, error)),
        }
    }

    fn response(
        &self,
        method: &'static str,
        params: &Value,
        timeout: Duration,
    ) -> Result<std::result::Result<Value, ApiError>> {
        let mut socket = self.connect(Some(timeout))?;
        let id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let request = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        write_json_line(&mut socket, &request, method, &self.socket_path)?;
        let mut reader = BufReader::new(socket);
        loop {
            let line = read_line(&mut reader, method, &self.socket_path)?;
            if line.is_empty() {
                return Err(Error::Protocol {
                    operation: method.to_owned(),
                    detail: "herdr-core closed the connection before it answered",
                });
            }
            let message: Message = serde_json::from_slice(&line).map_err(|source| Error::Json {
                operation: method,
                source,
            })?;
            if message.id == Some(json!(id)) {
                return answer(message, method);
            }
        }
    }

    fn connect(&self, timeout: Option<Duration>) -> Result<UnixStream> {
        let socket = UnixStream::connect(&self.socket_path).map_err(|source| Error::Io {
            operation: "connect to herdr-core",
            path: self.socket_path.clone(),
            source,
        })?;
        socket
            .set_read_timeout(timeout)
            .and_then(|()| socket.set_write_timeout(timeout))
            .map_err(|source| Error::Io {
                operation: "configure the herdr-core socket",
                path: self.socket_path.clone(),
                source,
            })?;
        Ok(socket)
    }

    /// Subscribes to the events of the terminals `params` match, and returns the snapshot of
    /// those terminals with the events that follow it; `None` when `params` name a terminal
    /// herdr-core does not know. `canceller` ends the subscription.
    fn subscribe(
        &self,
        params: &Value,
        canceller: &EventCanceller,
    ) -> Result<Option<(Vec<Terminal>, Subscription)>> {
        const OPERATION: &str = "events.subscribe";
        let mut socket = self.connect(None)?;
        let tie = canceller
            .tie(&socket)
            .map_err(|source| Error::Io {
                operation: "configure herdr-core event cancellation",
                path: self.socket_path.clone(),
                source,
            })?
            .ok_or(Error::Cancelled)?;
        let id = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let request = json!({"jsonrpc": "2.0", "id": id, "method": OPERATION, "params": params});
        let or_cancellation = |error| {
            if canceller.is_cancelled() {
                Error::Cancelled
            } else {
                error
            }
        };
        write_json_line(&mut socket, &request, OPERATION, &self.socket_path)
            .map_err(or_cancellation)?;
        let mut subscription = Subscription {
            reader: BufReader::new(socket),
            tie,
            socket_path: self.socket_path.clone(),
        };
        let message = subscription
            .next_message(OPERATION)?
            .ok_or(Error::Cancelled)?;
        if message.id != Some(json!(id)) {
            return Err(Error::Protocol {
                operation: OPERATION.to_owned(),
                detail: "herdr-core sent something else before it answered the subscription",
            });
        }
        match answer::<Snapshot>(message, OPERATION)? {
            Ok(snapshot) => Ok(Some((snapshot.terminals, subscription))),
            Err(error) if error.is("not_found") => Ok(None),
            Err(error) => Err(refusal(OPERATION, error)),
        }
    }
}

impl Subscription {
    /// The next event; `None` once the stream's canceller ended it. A subscriber that fell
    /// behind is an error: it subscribes again for a fresh snapshot.
    fn next_event(&mut self) -> Result<Option<Event>> {
        const OPERATION: &str = "read herdr-core events";
        while let Some(message) = self.next_message(OPERATION)? {
            match message.method.as_deref() {
                Some("event") => {
                    let params = message.params.unwrap_or_default();
                    return Event::deserialize(params)
                        .map(Some)
                        .map_err(|source| Error::Json {
                            operation: OPERATION,
                            source,
                        });
                }
                Some("missed") => {
                    return Err(Error::Protocol {
                        operation: OPERATION.to_owned(),
                        detail: "the subscriber fell behind herdr-core's events",
                    });
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// The next line herdr-core sends; `None` once the canceller ended the stream.
    fn next_message(&mut self, operation: &'static str) -> Result<Option<Message>> {
        let line = match read_line(&mut self.reader, operation, &self.socket_path) {
            Ok(line) => line,
            Err(_) if self.tie.is_cancelled() => return Ok(None),
            Err(error) => return Err(error),
        };
        if line.is_empty() {
            if self.tie.is_cancelled() {
                return Ok(None);
            }
            return Err(Error::Protocol {
                operation: operation.to_owned(),
                detail: "herdr-core closed the event stream",
            });
        }
        serde_json::from_slice(&line)
            .map(Some)
            .map_err(|source| Error::Json { operation, source })
    }
}

/// The result of an answer, or herdr-core's refusal.
fn answer<T: DeserializeOwned>(
    message: Message,
    method: &'static str,
) -> Result<std::result::Result<T, ApiError>> {
    match (message.result, message.error) {
        (Some(result), None) => parse(result, method).map(Ok),
        (None, Some(error)) => Ok(Err(error)),
        _ => Err(Error::Protocol {
            operation: method.to_owned(),
            detail: "herdr-core answered with neither one result nor one error",
        }),
    }
}

fn parse<T: DeserializeOwned>(value: Value, method: &'static str) -> Result<T> {
    serde_json::from_value(value).map_err(|source| Error::Json {
        operation: method,
        source,
    })
}

fn refusal(method: &'static str, error: ApiError) -> Error {
    Error::Herdr {
        operation: method,
        message: error.message,
    }
}

impl AgentPort for HcClient {
    /// herdr-core keeps no focus: herdr-app keeps it on each screen.
    fn session_snapshot(&self) -> Result<SessionSnapshot> {
        Ok(SessionSnapshot::default())
    }

    fn list_agents(&self) -> Result<Vec<Agent>> {
        let listed: Listed = self.call("terminal.list", &json!({}))?;
        Ok(listed
            .terminals
            .iter()
            .filter_map(Terminal::agent)
            .collect())
    }

    fn get_agent(&self, pane_id: &PaneId) -> Result<Option<Agent>> {
        Ok(self.terminal(pane_id)?.as_ref().and_then(Terminal::agent))
    }

    fn pane_process_info(&self, pane_id: &PaneId) -> Result<PaneProcessInfo> {
        let terminal: Terminal = self.call("terminal.get", &json!({"terminal": pane_id.0}))?;
        Ok(terminal.process_info())
    }

    /// herdr-core tells whether the agent went working within a few seconds; an agent that
    /// blocked on its user at once started too, as Herdr's wait judges it.
    fn prompt_agent(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        let sent = self.send(pane_id, text)?;
        if sent.started != Some(false) {
            return Ok(());
        }
        let started = self
            .get_agent(pane_id)?
            .is_some_and(|agent| agent.agent_status.started_on_prompt());
        if started {
            return Ok(());
        }
        Err(Error::AgentNotStarted {
            message: format!(
                "the agent stayed {} after the prompt",
                sent.status.as_deref().unwrap_or("silent")
            ),
        })
    }

    fn submit_prompt(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        self.send(pane_id, text).map(|_| ())
    }
}

impl AgentHost for HcClient {
    fn forward_events(
        &self,
        canceller: &EventCanceller,
        send: &mut dyn FnMut(HerdrEvent) -> bool,
    ) -> Result<()> {
        let Some((snapshot, mut events)) = self.subscribe(&json!({}), canceller)? else {
            return Err(Error::Protocol {
                operation: "events.subscribe".to_owned(),
                detail: "herdr-core found no terminal for a subscription that names none",
            });
        };
        let mut roster = Roster::new(snapshot);
        while let Some(event) = events.next_event()? {
            if let Some(event) = roster.observe(&event)?
                && !send(event)
            {
                break;
            }
        }
        Ok(())
    }

    /// The stream ends with the agent's program, at once when it ended already or when
    /// herdr-core no longer knows its terminal.
    fn forward_agent_status(
        &self,
        pane_id: &PaneId,
        canceller: &EventCanceller,
        send: &mut dyn FnMut(AgentStatus) -> bool,
    ) -> Result<()> {
        let Some((snapshot, mut events)) =
            self.subscribe(&json!({"terminal": pane_id.0}), canceller)?
        else {
            return Ok(());
        };
        let Some(terminal) = snapshot.iter().find(|terminal| terminal.is_live()) else {
            // Its program ended already: herdr-core would tell nothing more until it removes
            // the terminal.
            return Ok(());
        };
        if let Some(status) = &terminal.status
            && !send(status.agent_status())
        {
            return Ok(());
        }
        while let Some(event) = events.next_event()? {
            if event.ends_program() {
                break;
            }
            if event.kind == "status" && !send(status_of(&event.data::<StatusChange>()?)) {
                break;
            }
        }
        Ok(())
    }

    fn report_agent_session(&self, _pane_id: &PaneId, _report: &SessionReport<'_>) -> Result<()> {
        Err(Error::Unsupported {
            operation: "report the agent's session",
            reason: "herdr-core keeps no agent session",
        })
    }
}

#[cfg(test)]
#[path = "client.tests.rs"]
mod tests;
