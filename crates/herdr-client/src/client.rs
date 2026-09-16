//! Herdr newline-delimited JSON socket client.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::protocol::{
    Agent, AgentPrompter, AgentStatus, EntrypointId, HerdrEvent, HerdrReader, HerdrWriter,
    OpenPluginPane, PaneId, PluginPane, SessionSnapshot, TabId, WorkspaceId, method,
};
use crate::{Error, Result};

const RESPONSE_LIMIT: u64 = 16 * 1024 * 1024;
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(50);
static EVENT_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A synchronous client for the local Herdr socket.
#[derive(Clone, Debug)]
pub struct HerdrClient {
    socket_path: PathBuf,
    plugin_id: String,
    state_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Response {
    id: String,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct EventEnvelope {
    event: String,
    data: Value,
}

#[derive(Debug, Deserialize)]
struct FocusEvent {
    pane_id: PaneId,
}

#[derive(Debug, Deserialize)]
struct AgentDetectedEvent {
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    released: bool,
    #[serde(default)]
    final_status: Option<AgentStatus>,
}

#[derive(Debug, Deserialize)]
struct AgentStatusChangedEvent {
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    #[serde(default)]
    agent: Option<String>,
    agent_status: AgentStatus,
}

#[derive(Debug, Deserialize)]
struct PaneWire {
    #[serde(rename = "pane_id")]
    pane: PaneId,
    #[serde(rename = "tab_id")]
    tab: TabId,
    #[serde(rename = "workspace_id")]
    workspace: WorkspaceId,
}

#[derive(Debug, Deserialize)]
struct PluginPaneWire {
    entrypoint: EntrypointId,
    pane: PaneWire,
}

#[derive(Debug, Deserialize)]
struct PaneReadWire {
    text: String,
}

/// The reason that a Herdr event stream ended normally.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventStreamEnd {
    /// The event receiver disconnected.
    ReceiverDisconnected,
    /// A newly detected agent needs an additional status subscription.
    AgentPanesChanged(PaneId),
}

#[derive(Debug)]
struct HerdrEventStream {
    reader: BufReader<UnixStream>,
    agent_panes: Vec<PaneId>,
}

impl HerdrEventStream {
    fn forward(mut self, sender: &Sender<HerdrEvent>) -> Result<EventStreamEnd> {
        self.forward_with(|event| sender.send(event).is_ok())
    }

    fn forward_with(&mut self, mut send: impl FnMut(HerdrEvent) -> bool) -> Result<EventStreamEnd> {
        self.forward_while(|| true, &mut send)
    }

    fn forward_while(
        &mut self,
        mut should_continue: impl FnMut() -> bool,
        mut send: impl FnMut(HerdrEvent) -> bool,
    ) -> Result<EventStreamEnd> {
        loop {
            if !should_continue() {
                return Ok(EventStreamEnd::ReceiverDisconnected);
            }
            let Some(event) = self.read_event()? else {
                continue;
            };
            let new_agent_pane = self.new_agent_pane(&event);
            if !send(event) {
                return Ok(EventStreamEnd::ReceiverDisconnected);
            }
            if let Some(pane_id) = new_agent_pane {
                return Ok(EventStreamEnd::AgentPanesChanged(pane_id));
            }
        }
    }

    fn read_event(&mut self) -> Result<Option<HerdrEvent>> {
        let line = match read_line(&mut self.reader, "read Herdr event") {
            Ok(line) => line,
            Err(Error::Io { source, .. }) if is_temporary_read_error(&source) => return Ok(None),
            Err(error) => return Err(error),
        };
        if line.is_empty() {
            return Err(Error::Protocol {
                operation: "read Herdr event".to_owned(),
                detail: "Herdr closed the event stream",
            });
        }
        let envelope: EventEnvelope =
            serde_json::from_slice(&line).map_err(|source| Error::Json {
                operation: "read Herdr event",
                source,
            })?;
        parse_stream_event(envelope)
    }

    fn new_agent_pane(&self, event: &HerdrEvent) -> Option<PaneId> {
        let HerdrEvent::AgentDetected {
            pane_id, released, ..
        } = event
        else {
            return None;
        };
        (!released && !self.agent_panes.contains(pane_id)).then(|| pane_id.clone())
    }
}

fn is_temporary_read_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn parse_stream_event(event: EventEnvelope) -> Result<Option<HerdrEvent>> {
    Ok(match event.event.as_str() {
        "pane_focused" => {
            let value: FocusEvent = parse_event(event.data, "read Herdr focus event")?;
            Some(HerdrEvent::PaneFocused(value.pane_id))
        }
        "pane_agent_detected" => {
            let value: AgentDetectedEvent =
                parse_event(event.data, "read Herdr agent detection event")?;
            Some(HerdrEvent::AgentDetected {
                pane_id: value.pane_id,
                workspace_id: value.workspace_id,
                agent: value.agent,
                released: value.released,
                final_status: value.final_status,
            })
        }
        "pane_agent_status_changed" => {
            let value: AgentStatusChangedEvent =
                parse_event(event.data, "read Herdr agent status event")?;
            Some(HerdrEvent::AgentStatusChanged {
                pane_id: value.pane_id,
                workspace_id: value.workspace_id,
                agent: value.agent,
                status: value.agent_status,
            })
        }
        _ => None,
    })
}

impl HerdrClient {
    /// Read terminal styling so placeholder text can be distinguished from input.
    pub fn read_agent_screen_ansi(&self, pane_id: &PaneId) -> Result<String> {
        self.agent_screen(pane_id, true)
    }

    fn agent_screen(&self, pane_id: &PaneId, styled: bool) -> Result<String> {
        let result = self.request(
            method::AGENT_READ,
            &json!({
                "target": pane_id.0,
                "source": "visible",
                "format": if styled { "ansi" } else { "text" },
                "strip_ansi": !styled,
            }),
        )?;
        let read: PaneReadWire = Self::parse(&result, "read", method::AGENT_READ)?;
        Ok(read.text)
    }

    /// Inspect the processes currently owning a pane, without reading their environment.
    pub fn pane_process_info(&self, pane_id: &PaneId) -> Result<crate::protocol::PaneProcessInfo> {
        let result = self.request("pane.process_info", &json!({"pane_id": pane_id.0}))?;
        Self::parse(&result, "process_info", "pane.process_info")
    }

    /// Build a client for an explicitly configured Herdr connection.
    pub fn new(socket_path: PathBuf, plugin_id: String, state_dir: PathBuf) -> Self {
        Self {
            socket_path,
            plugin_id,
            state_dir,
        }
    }

    /// Build a client from the values injected into a plugin process.
    pub fn from_env() -> Result<Self> {
        Ok(Self::new(
            required_path("HERDR_SOCKET_PATH")?,
            required_string("HERDR_PLUGIN_ID")?,
            required_path("HERDR_PLUGIN_STATE_DIR")?,
        ))
    }

    /// Stream reviewer-relevant events until the event receiver disconnects.
    pub fn forward_events(
        &self,
        sender: &Sender<HerdrEvent>,
        agent_panes: &[PaneId],
    ) -> Result<EventStreamEnd> {
        self.subscribe_events(agent_panes)?.forward(sender)
    }

    /// Stream reviewer-relevant events through a callback.
    pub fn forward_events_with(
        &self,
        agent_panes: &[PaneId],
        send: impl FnMut(HerdrEvent) -> bool,
    ) -> Result<EventStreamEnd> {
        self.subscribe_events(agent_panes)?.forward_with(send)
    }

    /// Stream reviewer events until the caller requests cancellation.
    pub fn forward_events_while(
        &self,
        agent_panes: &[PaneId],
        should_continue: impl FnMut() -> bool,
        send: impl FnMut(HerdrEvent) -> bool,
    ) -> Result<EventStreamEnd> {
        let mut stream = self.subscribe_events(agent_panes)?;
        stream
            .reader
            .get_ref()
            .set_read_timeout(Some(EVENT_CANCELLATION_POLL_INTERVAL))
            .map_err(|source| Error::Io {
                operation: "configure Herdr event cancellation",
                path: self.socket_path.clone(),
                source,
            })?;
        stream.forward_while(should_continue, send)
    }

    fn subscribe_events(&self, agent_panes: &[PaneId]) -> Result<HerdrEventStream> {
        let mut socket = self.connect(None)?;
        let request_id = format!(
            "progressive-reviewer-events-{}-{}",
            std::process::id(),
            EVENT_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let request = json!({
            "id": &request_id,
            "method": "events.subscribe",
            "params": {"subscriptions": event_subscriptions(agent_panes)}
        });
        write_json_line(&mut socket, &request, "subscribe to Herdr events")?;
        let mut reader = BufReader::new(socket);
        let response: Response =
            serde_json::from_slice(&read_line(&mut reader, "subscribe to Herdr events")?).map_err(
                |source| Error::Json {
                    operation: "subscribe to Herdr events",
                    source,
                },
            )?;
        if let Err(error) = validate_response(response, &request_id, "subscribe to Herdr events")? {
            return Err(Error::Herdr {
                operation: "subscribe to Herdr events",
                message: error.message,
            });
        }
        Ok(HerdrEventStream {
            reader,
            agent_panes: agent_panes.to_vec(),
        })
    }

    fn request(&self, operation: &'static str, params: &Value) -> Result<Value> {
        match self.response(operation, params)? {
            Ok(result) => Ok(result),
            Err(error) => Err(Error::Herdr {
                operation,
                message: error.message,
            }),
        }
    }

    fn response(
        &self,
        operation: &'static str,
        params: &Value,
    ) -> Result<std::result::Result<Value, ApiError>> {
        let mut socket = self.connect(Some(SOCKET_TIMEOUT))?;
        let request = json!({
            "id": "progressive-reviewer",
            "method": operation,
            "params": params,
        });
        write_json_line(&mut socket, &request, operation)?;
        let response: Response =
            serde_json::from_slice(&read_line(&mut BufReader::new(socket), operation)?)
                .map_err(|source| Error::Json { operation, source })?;
        validate_response(response, "progressive-reviewer", operation)
    }

    fn connect(&self, timeout: Option<Duration>) -> Result<UnixStream> {
        let socket = UnixStream::connect(&self.socket_path).map_err(|source| Error::Io {
            operation: "connect to Herdr",
            path: self.socket_path.clone(),
            source,
        })?;
        socket
            .set_read_timeout(timeout)
            .and_then(|()| socket.set_write_timeout(timeout))
            .map_err(|source| Error::Io {
                operation: "configure Herdr socket",
                path: self.socket_path.clone(),
                source,
            })?;
        Ok(socket)
    }

    fn parse<T: for<'de> Deserialize<'de>>(
        value: &Value,
        field: &'static str,
        operation: &'static str,
    ) -> Result<T> {
        serde_json::from_value(value.get(field).cloned().ok_or(Error::Protocol {
            operation: operation.to_owned(),
            detail: "Herdr response did not contain the required field",
        })?)
        .map_err(|source| Error::Json { operation, source })
    }

    fn pane_path(&self, workspace_id: &WorkspaceId) -> PathBuf {
        let mut key = String::with_capacity(workspace_id.0.len() * 2);
        for byte in workspace_id.0.bytes() {
            use std::fmt::Write as _;
            write!(key, "{byte:02x}").expect("writing to a string cannot fail");
        }
        self.state_dir.join("panes").join(format!("{key}.json"))
    }

    fn save_pane(&self, pane: &PluginPane) -> Result<()> {
        let target = self.pane_path(&pane.workspace_id);
        let directory = target.parent().expect("a pane record has a parent");
        fs::create_dir_all(directory).map_err(|source| Error::Io {
            operation: "create Herdr pane state",
            path: directory.to_owned(),
            source,
        })?;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).map_err(|source| {
            Error::Io {
                operation: "secure Herdr pane state",
                path: directory.to_owned(),
                source,
            }
        })?;
        let temporary = target.with_extension(format!("tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec(pane).map_err(|source| Error::Json {
            operation: "store Herdr pane state",
            source,
        })?;
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|source| Error::Io {
                operation: "store Herdr pane state",
                path: temporary.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&temporary, &target))
            .map_err(|source| Error::Io {
                operation: "store Herdr pane state",
                path: target,
                source,
            })
    }

    fn load_pane(&self, workspace_id: &WorkspaceId) -> Result<Option<PluginPane>> {
        let target = self.pane_path(workspace_id);
        let bytes = match fs::read(&target) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    operation: "read Herdr pane state",
                    path: target,
                    source,
                });
            }
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|source| Error::Json {
                operation: "read Herdr pane state",
                source,
            })
    }

    fn remove_pane(&self, pane_id: &PaneId) -> Result<()> {
        let directory = self.state_dir.join("panes");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(Error::Io {
                    operation: "scan Herdr pane state",
                    path: directory,
                    source,
                });
            }
        };
        for entry in entries.flatten() {
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            if serde_json::from_slice::<PluginPane>(&bytes)
                .is_ok_and(|pane| pane.pane_id == *pane_id)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    fn synchronize_pane_size(&self, pane_id: &PaneId) -> Result<()> {
        // Herdr 0.9 can leave plugin panes at the parent PTY's dimensions.
        // A zero-distance resize applies the visible geometry without moving
        // the split or changing focus, including when retrying an earlier open.
        self.request(
            "pane.resize",
            &json!({"pane_id": pane_id.0, "direction": "left", "amount": 0.0}),
        )?;
        Ok(())
    }
}

fn parse_event<T: for<'de> Deserialize<'de>>(data: Value, operation: &'static str) -> Result<T> {
    serde_json::from_value(data).map_err(|source| Error::Json { operation, source })
}

fn event_subscriptions(agent_panes: &[PaneId]) -> Vec<Value> {
    let mut subscriptions = vec![
        json!({"type": "pane.focused"}),
        json!({"type": "pane.agent_detected"}),
    ];
    subscriptions.extend(agent_panes.iter().map(|pane_id| {
        json!({
            "type": "pane.agent_status_changed",
            "pane_id": pane_id.0,
        })
    }));
    subscriptions
}

fn validate_response(
    response: Response,
    expected_id: &str,
    operation: &'static str,
) -> Result<std::result::Result<Value, ApiError>> {
    if response.id != expected_id {
        return Err(Error::Protocol {
            operation: operation.to_owned(),
            detail: "Herdr returned a response with the wrong request ID",
        });
    }
    match (response.result, response.error) {
        (Some(result), None) => Ok(Ok(result)),
        (None, Some(error)) => Ok(Err(error)),
        _ => Err(Error::Protocol {
            operation: operation.to_owned(),
            detail: "Herdr returned neither one result nor one error",
        }),
    }
}

impl HerdrReader for HerdrClient {
    fn session_snapshot(&self) -> Result<SessionSnapshot> {
        let result = self.request(method::SESSION_SNAPSHOT, &json!({}))?;
        Self::parse(&result, "snapshot", method::SESSION_SNAPSHOT)
    }

    fn list_agents(&self) -> Result<Vec<Agent>> {
        let result = self.request(method::AGENT_LIST, &json!({}))?;
        Self::parse(&result, "agents", method::AGENT_LIST)
    }

    fn get_agent(&self, pane_id: &PaneId) -> Result<Option<Agent>> {
        match self.response(method::AGENT_GET, &json!({"target": pane_id.0}))? {
            Ok(result) => Self::parse(&result, "agent", method::AGENT_GET).map(Some),
            Err(error) if error.code.contains("not_found") => Ok(None),
            Err(error) => Err(Error::Herdr {
                operation: method::AGENT_GET,
                message: error.message,
            }),
        }
    }

    fn read_agent_screen(&self, pane_id: &PaneId) -> Result<String> {
        self.agent_screen(pane_id, false)
    }

    fn list_plugin_panes(&self, workspace_id: &WorkspaceId) -> Result<Vec<PluginPane>> {
        let Some(pane) = self.load_pane(workspace_id)? else {
            return Ok(Vec::new());
        };
        match self.response(method::PANE_GET, &json!({"pane_id": pane.pane_id.0}))? {
            Ok(_) => Ok(vec![pane]),
            Err(error) if error.code.contains("not_found") => {
                self.remove_pane(&pane.pane_id)?;
                Ok(Vec::new())
            }
            Err(error) => Err(Error::Herdr {
                operation: method::PANE_GET,
                message: error.message,
            }),
        }
    }
}

impl HerdrWriter for HerdrClient {
    fn open_plugin_pane(&self, request: &OpenPluginPane) -> Result<PluginPane> {
        let result = self.request(
            method::PLUGIN_PANE_OPEN,
            &json!({
                "plugin_id": self.plugin_id,
                "entrypoint": request.entrypoint,
                "placement": request.placement,
                "target_pane_id": request.target_pane_id,
                "cwd": request.cwd,
                "focus": request.focus,
            }),
        )?;
        let wire: PluginPaneWire = Self::parse(&result, "plugin_pane", method::PLUGIN_PANE_OPEN)?;
        let pane = PluginPane {
            pane_id: wire.pane.pane,
            tab_id: wire.pane.tab,
            workspace_id: wire.pane.workspace,
            entrypoint_id: wire.entrypoint,
        };
        self.save_pane(&pane)?;
        self.synchronize_pane_size(&pane.pane_id)?;
        Ok(pane)
    }

    fn focus_plugin_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.request(method::PLUGIN_PANE_FOCUS, &json!({"pane_id": pane_id.0}))?;
        self.synchronize_pane_size(pane_id)
    }

    fn focus_agent(&self, pane_id: &PaneId) -> Result<()> {
        self.request(method::AGENT_FOCUS, &json!({"target": pane_id.0}))?;
        Ok(())
    }

    fn close_plugin_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.request(method::PLUGIN_PANE_CLOSE, &json!({"pane_id": pane_id.0}))?;
        self.remove_pane(pane_id)
    }

    fn send_text(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        self.request(
            method::PANE_SEND_TEXT,
            &json!({"pane_id": pane_id.0, "text": text}),
        )?;
        Ok(())
    }

    fn send_keys(&self, pane_id: &PaneId, keys: &[&str]) -> Result<()> {
        self.request(
            method::PANE_SEND_KEYS,
            &json!({"pane_id": pane_id.0, "keys": keys}),
        )?;
        Ok(())
    }
}

impl AgentPrompter for HerdrClient {
    fn prompt_agent(&self, pane_id: &PaneId, text: &str) -> Result<()> {
        self.request(
            method::AGENT_PROMPT,
            &json!({"target": pane_id.0, "text": text}),
        )?;
        Ok(())
    }
}

fn required_path(name: &'static str) -> Result<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or(Error::Environment { name })
}

fn required_string(name: &'static str) -> Result<String> {
    env::var(name).map_err(|_| Error::Environment { name })
}

fn write_json_line(socket: &mut UnixStream, value: &Value, operation: &'static str) -> Result<()> {
    serde_json::to_writer(&mut *socket, value)
        .map_err(|source| Error::Json { operation, source })?;
    socket.write_all(b"\n").map_err(|source| Error::Io {
        operation,
        path: PathBuf::from("HERDR_SOCKET_PATH"),
        source,
    })
}

fn read_line<R: BufRead>(reader: &mut R, operation: &'static str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    Read::by_ref(reader)
        .take(RESPONSE_LIMIT + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|source| Error::Io {
            operation,
            path: PathBuf::from("HERDR_SOCKET_PATH"),
            source,
        })?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err(Error::Protocol {
            operation: operation.to_owned(),
            detail: "Herdr response exceeded 16 MiB",
        });
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "client.tests.rs"]
mod tests;
