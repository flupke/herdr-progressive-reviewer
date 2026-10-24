//! The daemon's JSON-RPC messages, as far as following a session needs them: the client's
//! requests, their answers, and the notifications of the sessions it joined.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::follow::{SessionEvent, SessionStatus, TurnEnd};

/// The ID of the client's `initialize` request.
pub(crate) const INITIALIZE: u64 = 1;
/// The ID of the client's `thread/resume` request, which joins the session.
pub(crate) const JOIN: u64 = 2;

/// The client's requests and its notification, in the order it sends them.
pub(crate) fn initialize() -> String {
    json!({"id": INITIALIZE, "method": "initialize", "params": {
        "clientInfo": {"name": "herdr-progressive-reviewer", "version": env!("CARGO_PKG_VERSION")},
        // The goal snapshots that tell a resume are part of the experimental API.
        "capabilities": {"experimentalApi": true},
    }})
    .to_string()
}

pub(crate) fn initialized() -> String {
    json!({"method": "initialized"}).to_string()
}

/// Joins `session` without its past turns: on a loaded session, this subscribes the client to
/// its turns; on another, it loads the session from its rollout first.
pub(crate) fn join(session: &str) -> String {
    json!({"id": JOIN, "method": "thread/resume",
        "params": {"threadId": session, "excludeTurns": true}})
    .to_string()
}

/// A message from the daemon.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Incoming {
    /// The answer to the client's request `id`: the session's status for a join, or the
    /// daemon's error.
    Answer {
        id: u64,
        result: Result<Option<SessionStatus>, String>,
    },
    /// What the daemon says of the session `session`.
    Notice { session: String, notice: Notice },
    /// Anything else, such as a request the daemon makes of every client, which the TUI
    /// answers, or a notification of no session.
    Other,
}

/// What the daemon says of a session.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Notice {
    /// What a follow reports as the daemon said it: a turn's start, prompt or end, or the
    /// session's status.
    Event(SessionEvent),
    /// The session's goal as it is when a client joins the session, or `cleared` when it has
    /// none: the daemon sends it to the session's clients on every join.
    GoalSnapshot,
}

#[derive(Deserialize)]
struct Message {
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Deserialize)]
struct JoinResult {
    thread: Option<StatusOf>,
}

#[derive(Deserialize)]
struct StatusOf {
    status: Status,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Status {
    NotLoaded,
    Idle,
    SystemError,
    Active,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnNotice {
    thread_id: String,
    turn: Turn,
}

#[derive(Deserialize)]
struct Turn {
    id: String,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemNotice {
    thread_id: String,
    turn_id: String,
    item: Item,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Item {
    UserMessage {
        content: Vec<Input>,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Input {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StatusNotice {
    thread_id: String,
    status: Status,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoalNotice {
    thread_id: String,
    /// The turn that changed the goal; none in a snapshot.
    #[serde(default)]
    turn_id: Option<String>,
}

impl From<SessionEvent> for Notice {
    fn from(event: SessionEvent) -> Self {
        Self::Event(event)
    }
}

impl From<Status> for SessionStatus {
    fn from(status: Status) -> Self {
        match status {
            Status::NotLoaded => Self::NotLoaded,
            Status::Idle => Self::Idle,
            Status::SystemError => Self::SystemError,
            Status::Active => Self::Active,
        }
    }
}

/// Reads one text frame from the daemon.
pub(crate) fn read(text: &str) -> Incoming {
    let Ok(message) = serde_json::from_str::<Message>(text) else {
        return Incoming::Other;
    };
    match (message.id.as_ref().and_then(Value::as_u64), message.method) {
        (Some(id), None) => Incoming::Answer {
            id,
            result: match message.error {
                Some(error) => Err(error.message),
                None => Ok(message
                    .result
                    .and_then(|result| serde_json::from_value::<JoinResult>(result).ok())
                    .and_then(|result| result.thread)
                    .map(|thread| thread.status.into())),
            },
        },
        (None, Some(method)) => notice(&method, message.params).unwrap_or(Incoming::Other),
        _ => Incoming::Other,
    }
}

fn notice(method: &str, params: Value) -> Option<Incoming> {
    let (session, notice) = match method {
        "turn/started" => {
            let notice: TurnNotice = serde_json::from_value(params).ok()?;
            let turn = notice.turn.id;
            (notice.thread_id, SessionEvent::TurnStarted { turn }.into())
        }
        "turn/completed" => {
            let notice: TurnNotice = serde_json::from_value(params).ok()?;
            let end = match notice.turn.status.as_str() {
                "completed" => TurnEnd::Completed,
                "interrupted" => TurnEnd::Interrupted,
                _ => TurnEnd::Failed,
            };
            let turn = notice.turn.id;
            (
                notice.thread_id,
                SessionEvent::TurnEnded { turn, end }.into(),
            )
        }
        "item/completed" => {
            let notice: ItemNotice = serde_json::from_value(params).ok()?;
            let Item::UserMessage { content } = notice.item else {
                return None;
            };
            let text = content
                .into_iter()
                .filter_map(|input| match input {
                    Input::Text { text } => Some(text),
                    Input::Other => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let turn = notice.turn_id;
            (notice.thread_id, SessionEvent::Prompt { turn, text }.into())
        }
        "thread/status/changed" => {
            let notice: StatusNotice = serde_json::from_value(params).ok()?;
            (
                notice.thread_id,
                SessionEvent::Status(notice.status.into()).into(),
            )
        }
        "thread/closed" => {
            let session = params.get("threadId")?.as_str()?.to_owned();
            (
                session,
                SessionEvent::Status(SessionStatus::NotLoaded).into(),
            )
        }
        "thread/goal/cleared" | "thread/goal/updated" => {
            let notice: GoalNotice = serde_json::from_value(params).ok()?;
            if notice.turn_id.is_some() {
                // A turn changed the goal: no client joined.
                return None;
            }
            (notice.thread_id, Notice::GoalSnapshot)
        }
        _ => return None,
    };
    Some(Incoming::Notice { session, notice })
}

#[cfg(test)]
#[path = "wire.tests.rs"]
mod tests;
